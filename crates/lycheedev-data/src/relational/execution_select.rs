//! A SELECT is a bounded stackless state machine. Its row cursor, correlations,
//! grouping and result owners survive child queries without native callbacks.
use super::*;
use execution::{
    Child, Context, Control, Correlation, Fields, Owners, Query, Row, Scope, Shared, SourceRef,
    Sources,
};
use std::cell::Cell;
async fn inputs<'a, 'w: 'a>(
    query: &Query<'a>,
    sources: Shared<'a, Sources<'a>>,
    scope: &Scope<'a>,
    control: &Control<'a, 'w>,
) -> Result<Vec<SourceRef<'a>>> {
    control.work(|w| {
        w.safety_retain((query.joins.len() + 1) * std::mem::size_of::<SourceRef<'a>>())
    })?;
    let mut result = Vec::with_capacity(query.joins.len() + 1);
    let mut aliases = BTreeSet::new();
    let mut width = 0usize;
    for (index, (table, alias, derived)) in
        std::iter::once((&query.table, &query.alias, &query.derived))
            .chain(query.joins.iter().map(|j| (&j.table, &j.alias, &j.derived)))
            .enumerate()
    {
        control.work(|w| w.spend())?;
        if !aliases.insert(identifier_lower(alias)) {
            return Err(binding());
        }
        let input = if derived.is_some() {
            let child = query.child(
                if index == 0 {
                    Child::Derived
                } else {
                    Child::Join(index - 1)
                },
                control,
            )?;
            let computed = control
                .query(child, sources.clone(), scope.select(sources.clone()))
                .await?;
            SourceRef::Owned(control.share(Source {
                columns: computed.columns,
                rows: computed.rows,
                complete: computed.complete,
            })?)
        } else {
            control.source(&sources, table)?
        };
        width = width.checked_add(input.columns.len()).ok_or_else(binding)?;
        if input.columns.is_empty() || width > 4096 {
            return Err(binding());
        }
        let mut names = BTreeSet::new();
        for name in &input.columns {
            if name.is_empty() || !names.insert(identifier_lower(name)) {
                return Err(binding());
            }
        }
        result.push(input);
    }
    Ok(result)
}
async fn count<'a, 'w: 'a>(
    term: &Option<Term>,
    default: usize,
    context: &Context<'a>,
    control: &Control<'a, 'w>,
) -> Result<usize> {
    let Some(term) = term else {
        return Ok(default);
    };
    let _temporary = control.temporary_scope();
    let fields = Fields::Borrowed(subquery::Fields {
        columns: &[],
        alias: ColumnOwners::Single(""),
    });
    let value = evaluation::eval_async(term, &Row::Empty, &fields, context, control).await?;
    let n = int(&value).filter(|n| *n >= 0).ok_or_else(type_error)?;
    usize::try_from(n).map_err(|_| budget())
}
async fn project<'a, 'w: 'a>(
    outputs: &[Term],
    ordering: &[(Term, bool, Option<bool>)],
    row: &Row<'a>,
    fields: &Fields<'a>,
    context: &Context<'a>,
    bounded: &mut Option<topk::TopRows>,
    result: &mut Vec<results::Output>,
    control: &Control<'a, 'w>,
) -> Result<()> {
    let mut projected = Vec::with_capacity(outputs.len());
    for term in outputs {
        projected.push(evaluation::eval_async(term, row, fields, context, control).await?);
    }
    let mut order = Vec::with_capacity(ordering.len());
    for (term, _, _) in ordering {
        order.push(evaluation::eval_async(term, row, fields, context, control).await?);
    }
    let output = (projected, order);
    control.work(|w| {
        if let Some(bounded) = bounded {
            bounded.add(output, w)
        } else {
            w.retain(results::output_cost(&output, 64)?)?;
            result.push(output);
            Ok(())
        }
    })
}
pub(super) async fn run<'a, 'w: 'a>(
    query: Query<'a>,
    sources: Shared<'a, Sources<'a>>,
    scope: Scope<'a>,
    projection_mode: ProjectionMode,
    control: &Control<'a, 'w>,
) -> Result<QueryResult> {
    let complete = control.share(Cell::new(true))?;
    let mut context = Context {
        scope: if projection_mode == ProjectionMode::NamedSource {
            scope.select(sources.clone())
        } else {
            scope.clone()
        },
        group: None,
        complete: complete.clone(),
    };
    let prepared = if projection_mode == ProjectionMode::NamedSource {
        Some(inputs(&query, sources.clone(), &scope, control).await?)
    } else {
        None
    };
    let base = if let Some(inputs) = &prepared {
        inputs.first().ok_or_else(binding)?.clone()
    } else {
        control.source(&sources, &query.table)?
    };
    let joined = if query.joins.is_empty() {
        None
    } else {
        Some(
            joins::execution::prepare(
                query.clone(),
                prepared.as_deref().ok_or_else(binding)?,
                &context,
                control,
            )
            .await?,
        )
    };
    let ordinal_owners = if projection_mode == ProjectionMode::CompoundOrdinals {
        let count = base.columns.len();
        control.work(|w| {
            if let Some(shared) = w.shared {
                shared
                    .safety_only()
                    .charge(crate::resource::Cost::retained(
                        count
                            .checked_mul(std::mem::size_of::<String>() + 20)
                            .ok_or_else(budget)?,
                    ))?;
            }
            Ok(())
        })?;
        Some(control.share((0..count).map(|i| i.to_string()).collect::<Vec<_>>())?)
    } else {
        None
    };
    let fields = if let Some(plan) = &joined {
        plan.fields.clone()
    } else {
        Fields::new(
            base.clone(),
            if let Some(owners) = ordinal_owners {
                Owners::Columns(owners)
            } else {
                Owners::Query(query.clone())
            },
        )
    };
    let source_complete = match &fields {
        Fields::Owned { source, .. } => source.complete,
        Fields::Borrowed(_) => return Err(binding()),
    };
    if projection_mode == ProjectionMode::NamedSource {
        context.scope.fields = fields.clone();
    }
    let mut ordering = query.order.clone();
    let mut columns = Vec::new();
    let mut outputs = Vec::new();
    for p in &query.projection {
        if let Some(term) = &p.expr
            && !matches!(term, Term::Star(_))
        {
            binding::bind_async(term, &fields, true, &context, control).await?;
            outputs.push(term.clone());
            columns.push(if p.name.is_empty() {
                if let Term::Column(name) = term {
                    let f = fields.view();
                    subquery::display_column(f.columns, f.alias, name, context.view().scope.outer)?
                        .unwrap_or_else(|| format!("column_{}", columns.len() + 1))
                } else {
                    format!("column_{}", columns.len() + 1)
                }
            } else {
                p.name.clone()
            });
        } else {
            let requested = if let Some(Term::Star(qualifier)) = &p.expr {
                if !p.name.is_empty() {
                    return Err(binding());
                }
                qualifier
            } else {
                &p.name
            };
            let mut selected = 0usize;
            let f = fields.view();
            for (ordinal, column) in f.columns.iter().enumerate() {
                if !requested.is_empty() && !equal_fold(f.alias.owner(ordinal), requested) {
                    continue;
                }
                selected += 1;
                outputs.push(if projection_mode == ProjectionMode::CompoundOrdinals {
                    control.work(|w| w.spend())?;
                    Term::Ordinal(ordinal)
                } else {
                    Term::Column(ColumnRef::qualified(f.alias.owner(ordinal), column.clone()))
                });
                columns.push(column.clone());
            }
            if selected == 0 {
                return Err(binding());
            }
        }
    }
    if let Some(term) = &query.filter {
        binding::bind_async(term, &fields, false, &context, control).await?;
    }
    if let Some(term) = &query.having {
        binding::bind_async(term, &fields, true, &context, control).await?;
    }
    for key in &query.groups {
        binding::bind_async(key, &fields, false, &context, control).await?;
    }
    for (term, _, _) in &mut ordering {
        let output = if let Term::Column(name) = &*term
            && name.qualifier.is_empty()
        {
            let mut matched = columns
                .iter()
                .enumerate()
                .filter(|(_, c)| equal_fold(c, &name.name));
            let found = matched.next().map(|(i, _)| i);
            if matched.next().is_some() {
                return Err(binding());
            }
            found
        } else {
            None
        };
        if let Some(index) = output {
            *term = outputs[index].clone();
        }
        binding::bind_async(term, &fields, true, &context, control).await?;
    }
    count(&query.limit, usize::MAX, &context, control).await?;
    count(&query.offset, 0, &context, control).await?;
    let grouped = !query.groups.is_empty()
        || query.having.is_some()
        || outputs.iter().any(aggregate::contains)
        || ordering.iter().any(|(t, _, _)| aggregate::contains(t));
    let group_mapping = if grouped {
        let f = fields.view();
        Some(control.share(
            control.work(|w| subquery::group_mapping(&query.groups, f.columns, f.alias, w))?,
        )?)
    } else {
        None
    };
    let group_context = Context {
        group: group_mapping.map(|mapping| Correlation {
            fields: fields.clone(),
            mapping,
        }),
        ..context.clone()
    };
    let mut grouping = if grouped {
        Some(
            aggregate::Grouping::new_async(
                &query.groups,
                &mut outputs,
                &mut ordering,
                query.having.as_ref(),
                &columns,
                &group_context,
                control,
            )
            .await?,
        )
    } else {
        None
    };
    if control.planning() {
        return Ok(QueryResult {
            plan: None,
            columns,
            rows: Vec::new(),
            complete: source_complete && complete.get(),
        });
    }
    let selected = if joined.is_none()
        && control.work(|w| Ok(provider::find(&base, w).is_some()))?
        && provider::can_project(&query, fields.view().columns, fields.view().alias)
    {
        let f = fields.view();
        Some(control.work(|w| provider::selected(&query, f.columns, f.alias, w))?)
    } else {
        None
    };
    let mut bounded =
        if !grouped && !query.distinct && query.limit.is_some() && query.order.len() <= 1 {
            let limit = count(&query.limit, 0, &context, control).await?;
            let offset = count(&query.offset, 0, &context, control).await?;
            topk::TopRows::new(&query, false, limit, offset)
        } else {
            None
        };
    control.work(|w| {
        w.safety_retain(
            (outputs.len() + ordering.len())
                .checked_mul(std::mem::size_of::<Scalar>())
                .ok_or_else(budget)?,
        )
    })?;
    let output_fields = if let Some(grouping) = &grouping {
        control.work(|w| {
            w.safety_retain(
                grouping
                    .columns()
                    .iter()
                    .map(|n| n.len() + std::mem::size_of::<String>())
                    .sum(),
            )
        })?;
        Fields::new(
            SourceRef::Owned(control.share(Source {
                columns: grouping.columns().to_vec(),
                rows: Vec::new(),
                complete: true,
            })?),
            Owners::Empty,
        )
    } else {
        fields.clone()
    };
    let output_context = if grouped { &group_context } else { &context };
    let mut result = Vec::new();
    enum Input<'c, 'a, 'w: 'a> {
        Join(joins::execution::Cursor<'c, 'a, 'w>),
        Rows(selection::execution::Cursor<'c, 'a, 'w>),
    }
    let mut input = if let Some(plan) = joined {
        Input::Join(joins::execution::Cursor::new(plan, control)?)
    } else {
        Input::Rows(selection::execution::open(
            base,
            selected.as_deref(),
            query.filter.as_ref(),
            &fields,
            control,
        )?)
    };
    loop {
        let row = match &mut input {
            Input::Join(cursor) => cursor.next(&context, control).await?,
            Input::Rows(cursor) => cursor.next()?,
        };
        let Some(row) = row else {
            break;
        };
        // The unjoined producer validates before joinRow's entry charge;
        // executeRows validates again after its own charge. Keep both turns,
        // including malformed-row versus exhausted-work error precedence.
        if matches!(input, Input::Rows(_)) {
            row.with(|r| {
                if r.len() != fields.view().columns.len() {
                    Err(binding())
                } else {
                    Ok(())
                }
            })?;
            if projection_mode == ProjectionMode::NamedSource {
                control.work(|w| w.spend())?;
            }
        }
        let _temporary = control.temporary_scope();
        control.work(|w| w.spend())?;
        row.with(|r| {
            if r.len() != fields.view().columns.len() {
                Err(binding())
            } else {
                Ok(())
            }
        })?;
        if let Some(term) = &query.filter
            && truth(&evaluation::eval_async(term, &row, &fields, &context, control).await?)?
                != Some(true)
        {
            continue;
        }
        if let Some(grouping) = &mut grouping {
            grouping.add_async(&row, &fields, &context, control).await?;
        } else {
            project(
                &outputs,
                &ordering,
                &row,
                &output_fields,
                output_context,
                &mut bounded,
                &mut result,
                control,
            )
            .await?;
        }
    }
    drop(input);
    if let Some(grouping) = grouping {
        let mut finish = grouping.into_finish(control)?;
        while let Some(row) = finish.next(&output_fields, &group_context).await? {
            project(
                &outputs,
                &ordering,
                &row,
                &output_fields,
                &group_context,
                &mut bounded,
                &mut result,
                control,
            )
            .await?;
            drop(row);
            finish.finish_row();
        }
    }
    if let Some(bounded) = bounded {
        result = control.work(|w| bounded.finish(w))?;
    }
    let limit = count(&query.limit, usize::MAX, &context, control).await?;
    let offset = count(&query.offset, 0, &context, control).await?;
    let rows =
        control.work(|w| results::finish(result, query.distinct, &ordering, limit, offset, w))?;
    Ok(QueryResult {
        plan: None,
        columns,
        rows,
        complete: source_complete && complete.get(),
    })
}
