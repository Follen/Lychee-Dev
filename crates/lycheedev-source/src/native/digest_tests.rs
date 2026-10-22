use super::*;

#[test]
fn streamed_digest_matches_previous_bytes_for_values_and_struct_order() {
    for value in [Value::Null,json!([]),json!({}),json!({"z":[null,true,false,0,-1,1.25,-0.0],"a":{"z":"界😀\r\n\"\\\u{0000}","a":""}})] {
        assert_eq!(digest(&value),hash(&serde_json::to_vec(&value).unwrap()));
    }
    #[derive(Serialize)]
    struct NoncanonicalOrder<'a>{z:&'a str,a:u64,optional:Option<bool>}
    let value=NoncanonicalOrder{z:"last field alphabetically",a:u64::MAX,optional:None};
    let bytes=serde_json::to_vec(&value).unwrap();
    assert!(bytes.starts_with(b"{\"z\":"));
    assert_eq!(digest(&value),hash(&bytes));
    assert_eq!(digest(&(&value,[1,2,3])),hash(&serde_json::to_vec(&(&value,[1,2,3])).unwrap()));
}

#[test]
fn streamed_digest_keeps_fixed_sink_storage_for_large_borrowed_input() {
    let text="界😀\"\r\n".repeat(131072);
    let value=json!({"text":text,"empty":null});
    assert_eq!(std::mem::size_of::<JsonDigestWriter>(),std::mem::size_of::<Sha256>());
    assert!(std::mem::size_of::<JsonDigestWriter>()<=256);
    assert_eq!(digest(&value),hash(&serde_json::to_vec(&value).unwrap()));
}

#[test]
#[should_panic(expected="serializable bounded facts")]
fn streamed_digest_preserves_original_serialization_failure_contract() {
    struct Failure;
    impl Serialize for Failure {
        fn serialize<S:serde::Serializer>(&self,_:S)->std::result::Result<S::Ok,S::Error> {
            Err(serde::ser::Error::custom("original serializer failure"))
        }
    }
    let _=digest(&Failure);
}
