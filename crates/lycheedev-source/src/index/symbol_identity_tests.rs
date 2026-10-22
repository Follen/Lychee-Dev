use super::*;
fn sample() -> (SourcePin, Symbol) {
    (
        SourcePin {
            repository: "elvui".into(),
            product: "main".into(),
            requested_ref: "fixture".into(),
            exact_commit: "a".repeat(40),
            parser_revision: git::PARSER.into(),
        },
        Symbol {
            id: String::new(),
            kind: "declaration".into(),
            name: "Run".into(),
            target: String::new(),
            category: "function".into(),
            confidence: "exact".into(),
            path: "Addon/Main.lua".into(),
            line: 12,
            end_line: 34,
            signature: "Run(value)".into(),
            scope: "Widget".into(),
            ordinal: 5,
        },
    )
}

#[test]
fn stable_ids_follow_current_fixed_vectors_and_identity_fields() {
    let (pin, symbol)=sample();
    assert_eq!(stable_id(&pin,&symbol),"SYM-c0efdd111d1017aff6d2cd73a4511fa0");
    assert_eq!(SymbolIdentity::for_document(&pin,&symbol.path).stable_id(&symbol),stable_id(&pin,&symbol));
    let original=stable_id(&pin,&symbol);
    for field in 0..9 {
        let (mut changed_pin,mut changed)=(pin.clone(),symbol.clone());
        match field {0=>changed_pin.repository.push('!'),1=>changed_pin.exact_commit.push('!'),2=>changed.path.push('!'),3=>changed.line+=1,4=>changed.end_line+=1,5=>changed.name.push('!'),6=>changed.category.push('!'),7=>changed.scope.push('!'),8=>changed.ordinal+=1,_=>unreachable!()}
        assert_ne!(stable_id(&changed_pin,&changed),original);
    }
}
