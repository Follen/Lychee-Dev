use super::*;
use emmylua_code_analysis::{
    FileId, GenericParam, GenericTplId, InFiled, LuaAliasCallKind, LuaAliasCallType, LuaArrayLen,
    LuaArrayType, LuaConditionalType, LuaGenericType, LuaInstanceType, LuaIntersectionType,
    LuaMappedType, LuaMultiLineUnion, LuaStringTplType, LuaTypeDeclId,
};
use emmylua_parser::{LuaSyntaxId, LuaSyntaxKind};
use rowan::TextRange;

fn selected() -> OwnedValue {
    // Independent frozen identities with deliberately identical public fields.
    // The public ingestion/producer regression remains in object_carrier_contract.
    OwnedValue {
        ty: LuaType::Object(Arc::new(LuaObjectType::new_with_fields(
            Default::default(),
            vec![],
        ))),
        evidence: Some(Arc::new(ValueEvidence::Object {
            binding: ObjectBinding {
                token: SessionToken(Arc::new(())),
                context: "context".into(),
                identity: "object".into(),
                epoch: "epoch".into(),
                view: ObjectViewKey {
                    context: "context".into(),
                    observation: 1,
                    object_id: 1,
                },
                site: BindingSite {
                    file_id: FileId::new(0),
                    syntax: LuaSyntaxId::new(LuaSyntaxKind::NameExpr.into(), Default::default()),
                },
                proof: "shape-proof".into(),
                complete: true,
            },
            fields: BTreeMap::new(),
        })),
    }
}
fn hash(value: &OwnedValue) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

#[test]
fn nominal_type_identity_survives_containers_and_rejects_erased_or_foreign_types() {
    struct Context;
    impl super::super::BindingContext for Context {
        fn identity(&self) -> &str {
            "nominal"
        }
        fn resolve(&self, _: BindingSite) -> super::super::BindingDecision<super::super::BoundValue> {
            super::super::BindingDecision::NotManaged
        }
    }
    let session =
        super::super::OwnedBindingSession::new_checked(Arc::new(Context), || Ok::<_, ()>(()))
            .unwrap();
    let foreign =
        super::super::OwnedBindingSession::new_checked(Arc::new(Context), || Ok::<_, ()>(()))
            .unwrap();
    let a = OwnedValue::nominal(&session, "catalog-a", "Duration");
    let same = OwnedValue::nominal(&session, "catalog-a", "Duration");
    assert_eq!(a, same);
    assert_eq!(hash(&a), hash(&same));
    assert_ne!(a, OwnedValue::plain(a.structural_type().clone()));
    assert_ne!(a, OwnedValue::nominal(&foreign, "catalog-a", "Duration"));
    assert_ne!(a, OwnedValue::nominal(&session, "catalog-b", "Duration"));
    assert_ne!(a, OwnedValue::nominal(&session, "catalog-a", "Curve"));
    assert!(a.nominal_belongs_to(&session));
    assert!(!a.nominal_belongs_to(&foreign));
    let pack = OwnedValue::pack(vec![a.clone()]);
    assert_eq!(pack.result_slot(0), Some(a));
}
fn wrap(ty: LuaType, children: Vec<OwnedValue>) -> OwnedValue {
    OwnedValue::with_structural_children(ty, children)
}
fn shells(value: &OwnedValue) -> Vec<(&'static str, OwnedValue)> {
    let ty = value.structural_type();
    let one = || vec![value.clone()];
    vec![
        (
            "array",
            wrap(
                LuaType::Array(Arc::new(LuaArrayType::from_base_type(ty.clone()))),
                one(),
            ),
        ),
        (
            "alias call",
            wrap(
                LuaType::Call(Arc::new(LuaAliasCallType::new(
                    LuaAliasCallKind::KeyOf,
                    vec![ty.clone()],
                ))),
                one(),
            ),
        ),
        (
            "generic",
            wrap(
                LuaType::Generic(Arc::new(LuaGenericType::new(
                    LuaTypeDeclId::global("Box"),
                    vec![ty.clone()],
                ))),
                one(),
            ),
        ),
        (
            "intersection",
            wrap(
                LuaType::Intersection(Arc::new(LuaIntersectionType::new(vec![ty.clone()]))),
                one(),
            ),
        ),
        (
            "table generic",
            wrap(LuaType::TableGeneric(Arc::new(vec![ty.clone()])), one()),
        ),
        (
            "type guard",
            wrap(LuaType::TypeGuard(Arc::new(ty.clone())), one()),
        ),
        (
            "variadic base",
            wrap(
                LuaType::Variadic(Arc::new(VariadicType::Base(ty.clone()))),
                one(),
            ),
        ),
        (
            "instance",
            wrap(
                LuaType::Instance(Arc::new(LuaInstanceType::new(
                    ty.clone(),
                    InFiled::new(FileId::new(1), TextRange::empty(1.into())),
                ))),
                one(),
            ),
        ),
        (
            "object",
            object_shell(vec![("field", value.clone())], vec![]),
        ),
        (
            "conditional",
            wrap(
                LuaType::Conditional(Arc::new(LuaConditionalType::new(
                    ty.clone(),
                    LuaType::Any,
                    LuaType::String,
                    LuaType::Integer,
                    vec![],
                    false,
                ))),
                vec![
                    OwnedValue::plain(LuaType::Integer),
                    OwnedValue::plain(LuaType::String),
                    OwnedValue::plain(LuaType::Any),
                    value.clone(),
                ],
            ),
        ),
        (
            "mapped",
            wrap(
                LuaType::Mapped(Arc::new(LuaMappedType::new(
                    (
                        GenericTplId::Type(0),
                        GenericParam::new(
                            "T".into(),
                            Some(ty.clone()),
                            Some(ty.clone()),
                            false,
                            None,
                        ),
                    ),
                    ty.clone(),
                    false,
                    false,
                ))),
                vec![value.clone(), value.clone(), value.clone()],
            ),
        ),
        (
            "string template",
            wrap(
                LuaType::StrTplRef(Arc::new(LuaStringTplType::new(
                    "pre",
                    "T",
                    GenericTplId::Type(0),
                    "post",
                    Some(ty.clone()),
                ))),
                one(),
            ),
        ),
        (
            "multiline union",
            wrap(
                LuaType::MultiLineUnion(Arc::new(LuaMultiLineUnion::new(vec![(
                    ty.clone(),
                    Some("selected".into()),
                )]))),
                one(),
            ),
        ),
    ]
}
fn object_shell(
    fields: Vec<(&str, OwnedValue)>,
    indices: Vec<(OwnedValue, OwnedValue)>,
) -> OwnedValue {
    let object = LuaObjectType::new_with_fields(
        fields
            .iter()
            .map(|(key, value)| (LuaMemberKey::Name((*key).into()), value.ty.clone()))
            .collect(),
        indices
            .iter()
            .map(|(key, value)| (key.ty.clone(), value.ty.clone()))
            .collect(),
    );
    // Match the actual public producer order, not a separately sorted carrier.
    let mut children = Vec::new();
    for (key, value) in indices.into_iter().rev() {
        children.push(value);
        children.push(key);
    }
    for key in object.get_fields().keys() {
        children.push(
            fields
                .iter()
                .find(|(name, _)| LuaMemberKey::Name((*name).into()) == *key)
                .unwrap()
                .1
                .clone(),
        );
    }
    wrap(LuaType::Object(Arc::new(object)), children)
}

#[test]
fn nested_selected_union_identity_is_reflexive_order_independent_and_hash_consistent() {
    let a = selected();
    let b = selected();
    assert_eq!(a.ty, b.ty);
    assert_ne!(a, b);
    let ab = OwnedValue::union(vec![a.clone(), b.clone()]);
    let ba = OwnedValue::union(vec![b.clone(), a.clone()]);
    let foreign = OwnedValue::union(vec![a, selected()]);
    for (((name, left), (_, right)), (_, foreign)) in shells(&ab)
        .into_iter()
        .zip(shells(&ba))
        .zip(shells(&foreign))
    {
        assert_eq!(left, left.clone(), "{name} reflexivity");
        assert_eq!(left, right, "{name} union order");
        assert_eq!(
            hash(&left),
            hash(&right),
            "{name} equal values hash equally"
        );
        assert_ne!(left, foreign, "{name} foreign token remains distinct");
        assert_eq!(
            HashSet::from([left.clone(), left, right, foreign]).len(),
            2,
            "{name} cache identity"
        );
    }
}

#[test]
fn raw_union_preserves_multiplicity_without_losing_owned_token_identity() {
    let a = selected();
    let b = selected();
    let aab = OwnedValue::raw_union(vec![a.clone(), a.clone(), b.clone()]);
    let baa = OwnedValue::raw_union(vec![b.clone(), a.clone(), a.clone()]);
    let abb = OwnedValue::raw_union(vec![a.clone(), b.clone(), b]);
    assert_eq!(aab.carried_union().unwrap().len(), 3);
    assert_eq!(aab, aab.clone());
    assert_eq!(aab, baa);
    assert_eq!(hash(&aab), hash(&baa));
    assert_ne!(aab, abb);
    assert_eq!(HashSet::from([aab.clone(), baa, abb]).len(), 2);
    assert_eq!(
        OwnedValue::union(vec![aab.clone(), aab])
            .carried_union()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn structural_object_keys_and_index_positions_remain_identity_bearing() {
    let a = selected();
    let b = selected();
    let value = OwnedValue::raw_union(vec![a.clone(), b.clone()]);
    let key = OwnedValue::plain(LuaType::String);
    let left = object_shell(
        vec![("a", value.clone()), ("b", a.clone())],
        vec![(key.clone(), value.clone())],
    );
    let reordered = object_shell(
        vec![("b", a.clone()), ("a", value.clone())],
        vec![(key.clone(), value.clone())],
    );
    assert_eq!(left, reordered);
    assert_eq!(hash(&left), hash(&reordered));
    assert_ne!(
        left,
        object_shell(
            vec![("b", value.clone()), ("a", a.clone())],
            vec![(key.clone(), value.clone())]
        )
    );
    assert_ne!(
        left,
        object_shell(
            vec![("a", value.clone()), ("b", a.clone())],
            vec![(value.clone(), key.clone())]
        )
    );
    assert_ne!(
        left,
        object_shell(vec![("a", value.clone()), ("b", b)], vec![(key, value)])
    );
}

#[test]
fn structural_shell_metadata_is_not_erased_by_owned_child_comparison() {
    let value = selected();
    let ty = value.structural_type();
    let pairs = [
        (
            LuaType::Array(Arc::new(LuaArrayType::new(ty.clone(), LuaArrayLen::Max(1)))),
            LuaType::Array(Arc::new(LuaArrayType::new(ty.clone(), LuaArrayLen::Min(1)))),
        ),
        (
            LuaType::Call(Arc::new(LuaAliasCallType::new(
                LuaAliasCallKind::KeyOf,
                vec![ty.clone()],
            ))),
            LuaType::Call(Arc::new(LuaAliasCallType::new(
                LuaAliasCallKind::Index,
                vec![ty.clone()],
            ))),
        ),
        (
            LuaType::Generic(Arc::new(LuaGenericType::new(
                LuaTypeDeclId::global("A"),
                vec![ty.clone()],
            ))),
            LuaType::Generic(Arc::new(LuaGenericType::new(
                LuaTypeDeclId::global("B"),
                vec![ty.clone()],
            ))),
        ),
        (
            LuaType::Instance(Arc::new(LuaInstanceType::new(
                ty.clone(),
                InFiled::new(FileId::new(1), TextRange::empty(1.into())),
            ))),
            LuaType::Instance(Arc::new(LuaInstanceType::new(
                ty.clone(),
                InFiled::new(FileId::new(2), TextRange::empty(1.into())),
            ))),
        ),
        (
            LuaType::StrTplRef(Arc::new(LuaStringTplType::new(
                "a",
                "T",
                GenericTplId::Type(0),
                "",
                Some(ty.clone()),
            ))),
            LuaType::StrTplRef(Arc::new(LuaStringTplType::new(
                "b",
                "T",
                GenericTplId::Type(0),
                "",
                Some(ty.clone()),
            ))),
        ),
        (
            LuaType::MultiLineUnion(Arc::new(LuaMultiLineUnion::new(vec![(
                ty.clone(),
                Some("a".into()),
            )]))),
            LuaType::MultiLineUnion(Arc::new(LuaMultiLineUnion::new(vec![(
                ty.clone(),
                Some("b".into()),
            )]))),
        ),
    ];
    for (a, b) in pairs {
        assert_ne!(wrap(a, vec![value.clone()]), wrap(b, vec![value.clone()]));
    }
    for flag in [false, true] {
        let a = LuaType::Conditional(Arc::new(LuaConditionalType::new(
            ty.clone(),
            ty.clone(),
            ty.clone(),
            ty.clone(),
            vec![],
            flag,
        )));
        let b = LuaType::Conditional(Arc::new(LuaConditionalType::new(
            ty.clone(),
            ty.clone(),
            ty.clone(),
            ty.clone(),
            vec![],
            !flag,
        )));
        assert_ne!(
            wrap(a, vec![value.clone(); 4]),
            wrap(b, vec![value.clone(); 4])
        );
        let param = || {
            (
                GenericTplId::Type(0),
                GenericParam::new("T".into(), None, None, false, None),
            )
        };
        let a = LuaType::Mapped(Arc::new(LuaMappedType::new(
            param(),
            ty.clone(),
            flag,
            false,
        )));
        let b = LuaType::Mapped(Arc::new(LuaMappedType::new(
            param(),
            ty.clone(),
            !flag,
            false,
        )));
        assert_ne!(wrap(a, vec![value.clone()]), wrap(b, vec![value.clone()]));
    }
}
