//! Declarations only. Execution is prohibited until migration completion.
use super::*;
use crate::capture::committed_tests::fixture::{self,Fixture};
#[path="navigation_fixture_data.rs"] mod data;
fn witness(fx:&Fixture,command:&str,flags:&[(&str,&str)],mut request:Value){
 request["mode"]=json!("static");let requested=(request["kind"]=="creature_display"&&request["display_id"]==0).then(||request["file_data_id"].clone());
 let old=crate::navigate::dispatch(&fx.invocation(command,flags)).unwrap();
 let mut session=Session::new(fx.invocation("data agent serve",&[])).unwrap();
 let runtime=tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
 let response=runtime.block_on(session.execute(Task::Navigate{request:serde_json::from_value(request).unwrap()},OperationControl::uncancelable(Instant::now()+Duration::from_secs(120)))).unwrap();
 let value=serde_json::to_value(&response).unwrap();let report=&value["report"];
 for (k,v) in report.as_object().unwrap(){if k=="requestedFileDataID"{assert_eq!(Some(v),requested.as_ref());}else{assert_eq!(v,&old.data[k],"{command}: {k}");}}
 if let Some(expected)=requested{assert_eq!(report["requestedFileDataID"],expected);}else{assert!(report.get("requestedFileDataID").is_none());}
 assert_eq!(report["complete"],old.complete);assert!(report["relations"].is_array());
 let capture=&value["capture"];let id=capture["id"].as_str().unwrap();assert_eq!(lycheedev_core::evidence::read_capture(&fx.home,id).unwrap(),*capture);
 let bytes=lycheedev_core::evidence::read_blob(&fx.home,&capture["blob"],16<<20).unwrap();let body:Value=serde_json::from_slice(&bytes).unwrap();assert_eq!(body["result"]["report"],*report);assert_eq!(capture["complete"],report["complete"]);assert_eq!(capture["truncated"],report["truncated"]);drop(response);session.close().unwrap();
}
#[test] fn all_twelve_tasks_preserve_business_and_full_capture(){
 let fx=Fixture::new(&fixture::items());witness(&fx,"data item get",&[("--item-id","7")],json!({"kind":"item_get","item_id":7}));
 let fx=Fixture::new(&data::models::tables());witness(&fx,"data item models",&[("--item-id","1")],json!({"kind":"item_models","item_id":1,"race_id":0,"gender":0}));
 let fx=Fixture::new(&data::textures::tables());witness(&fx,"data item textures",&[("--item-id","1")],json!({"kind":"item_textures","item_id":1}));
 let fx=Fixture::new(&data::geosets::tables());witness(&fx,"data item geosets",&[("--item-id","1")],json!({"kind":"item_geosets","item_id":1}));
 let fx=Fixture::new(&data::spell::tables());witness(&fx,"data spell info",&[("--spell-id","100"),("--max-depth","1")],json!({"kind":"spell_info","spell_id":100,"max_depth":1}));
 witness(&fx,"data spell auras",&[("--spell-id","100")],json!({"kind":"spell_auras","spell_id":100}));witness(&fx,"data spell summons",&[("--spell-id","100")],json!({"kind":"spell_summons","spell_id":100,"npc_id":0}));
 let rows=fixture::decor_rows();let fixture::Cell::U(id)=rows[0][0].clone() else{panic!("fixture ID")};let fx=Fixture::new(&[fixture::decors(&rows)]);witness(&fx,"data decor list",&[("--limit","1")],json!({"kind":"decor_list","limit":1}));witness(&fx,"data decor get",&[("--id",&id.to_string())],json!({"kind":"decor_get","field":"id","wanted":id}));
 let fx=Fixture::new(&data::creature::tables());witness(&fx,"data creature display",&[("--file-data-id","500")],json!({"kind":"creature_display","display_id":0,"file_data_id":500}));witness(&fx,"data creature model",&[("--file-data-id","500")],json!({"kind":"creature_model","file_data_id":500}));
 let fx=data::encounter::fixture(&[data::encounter::section(1,1,0,0,0,123)],true);witness(&fx,"data encounter get",&[("--journal-encounter-id","132")],json!({"kind":"encounter_get","encounter_id":132,"max_depth":0}));
}
#[test] fn empty_encounter_does_not_open_spell_tables(){let fx=data::encounter::fixture(&[],false);witness(&fx,"data encounter get",&[("--journal-encounter-id","132")],json!({"kind":"encounter_get","encounter_id":132,"max_depth":0}));}
#[test] fn schema_has_twelve_strict_variants(){let s=navigation::schema();assert_eq!(s["oneOf"].as_array().unwrap().len(),12);assert!(serde_json::from_value::<Task>(json!({"task":"navigate","request":{"kind":"sql","mode":"static"}})).is_err());}
