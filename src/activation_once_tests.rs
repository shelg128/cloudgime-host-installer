#[cfg(all(test, windows))]
mod tests {
    use super::*;
    // Executed in fresh child processes by the PostgreSQL/HTTP harness. No
    // endpoint override is compiled into the production activation entrypoint.
    #[test]
    fn isolated_activation_process_worker() {
        let Ok(root) = std::env::var("CG_ACTIVATION_PROCESS_FIXTURE") else { return };
        let root = std::path::PathBuf::from(root);
        assert!(root.to_string_lossy().contains("activation-e2e-"));
        let endpoint = std::env::var("CG_ACTIVATION_FIXTURE_HTTP").unwrap();
        assert!(endpoint.starts_with("http://127.0.0.1:"));
        let phase = std::env::var("CG_ACTIVATION_FIXTURE_PHASE").unwrap();
        unsafe { std::env::set_var("CLOUDGIME_PROVISIONING_ROOT",root.join("provisioning")); }
        fs::create_dir_all(root.join("moonlight/server")).unwrap();
        let identity=load_or_create_machine_identity(&root).unwrap();
        let license=if phase=="response-loss" {std::env::var("CG_ACTIVATION_FIXTURE_LICENSE").unwrap()}else{String::new()};
        if phase!="response-loss" {assert!(can_resume(&root).unwrap());assert!(license.is_empty());}
        let request=ProvisionRequest {activation_license:&license,expected_activation_license_kind:"always_on_host",machine_name:"ACTIVATION-E2E",hostname:"ACTIVATION-E2E",machine_identity:&identity.machine_identity,install_instance_id:&identity.install_instance_id,requested_device_id:"FIXTURE-03",host_id:"host-fixture-03",installer_version:"test"};
        let mut calls=0;
        let result=run_inner(&root,NODE_CONTROL_PLANE,&request,|_,payload|{
            calls+=1;
            if phase=="complete"||phase=="retry-files"||phase=="partial-write" {bail!("UNEXPECTED_NETWORK_AFTER_RESPONSE_SAVED")}
            let response=reqwest::blocking::Client::builder().redirect(reqwest::redirect::Policy::none()).timeout(std::time::Duration::from_secs(20)).build()?.post(&endpoint).header("Host","cloudgime.my.id").header("X-Forwarded-Proto","https").json(payload).send()?;
            if response.status()!=reqwest::StatusCode::OK {bail!("HTTP_{}",response.status())}
            Ok(response.json()?)
        });
        if phase=="response-loss" {assert!(result.is_err());assert_eq!(calls,1);assert_eq!(load(&root.join("provisioning")).unwrap().unwrap().phase,"REQUESTED");}
        else if phase=="partial-write" {assert!(result.is_err(),"fixture must interrupt credential publication");assert_eq!(calls,0);let intent=load(&root.join("provisioning")).unwrap().unwrap();assert_eq!(intent.phase,"RESPONSE_SAVED");assert!(intent.license.is_empty());}
        else {result.unwrap();assert_eq!(calls,0);let intent=load(&root.join("provisioning")).unwrap().unwrap();assert_eq!(intent.phase,"COMPLETE");assert!(intent.license.is_empty());assert!(!read_component_credential(&root,"host_app").unwrap().is_empty());assert!(!read_component_credential(&root,"sentinel").unwrap().is_empty());}
        println!("ACTIVATION_PROCESS_PHASE_PASS {phase} network_calls={calls}");
    }
}
