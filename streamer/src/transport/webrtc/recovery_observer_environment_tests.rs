use super::*;
#[test]
fn environment_activation_contract(){
 let case=std::env::var("CG_QA_ENV_GATE_TEST_CASE").unwrap_or_else(|_|"off".into());
 let first=ObserverBuilder::from_environment();
 if case=="on"{assert!(first.is_some(),"valid explicit process environment");assert!(ObserverBuilder::from_environment().is_none(),"one collector per process");let tap=&first.as_ref().unwrap().tap;tap.record(0,1,Data::ReadError);tap.close();std::thread::sleep(std::time::Duration::from_millis(150));}
 else{assert!(first.is_none(),"absent/invalid gate must not activate");}
}
