use super::*;

#[actix_web::test]
async fn claim_cancel_revokes_all_devices_but_not_another_claim() {
    let (app,_root)=fixture().await;
    let a=launch(&app).await;
    let (_,ticket)=app.consume_android_native_launch_token_with_stream_ticket(&a.launch_token).await.unwrap();
    let other=launch(&app).await;
    let (_,other_ticket)=app.consume_android_native_launch_token_with_stream_ticket(&other.launch_token).await.unwrap();
    let handoff=Uuid::new_v4().to_string();
    {
        let mut states=app.inner.android_native_session_lifecycle.write().await;
        states.get_mut(&a.session_id).unwrap().rental_binding=Some(AndroidNativeRentalBindingRecord{claim_id:"claim-cancel".into(),handoff_binding_id:handoff.clone(),prepare_operation_id:None,rental_session_id:None});
        states.get_mut(&other.session_id).unwrap().rental_binding=Some(AndroidNativeRentalBindingRecord{claim_id:"claim-other".into(),handoff_binding_id:Uuid::new_v4().to_string(),prepare_operation_id:None,rental_session_id:None});
    }
    // The test issues distinct roots with the same local account; reinsert the first
    // already-issued ticket to test claim isolation, not legacy single-user eviction.
    app.inner.android_native_stream_tickets.write().await.insert(ticket.stream_ticket.clone(),ticket.clone());
    assert!(app.revoke_unstarted_claim_root(&a.session_id,"wrong",&handoff).await.is_err());
    assert!(app.peek_android_native_stream_ticket(&ticket.stream_ticket).await.is_ok());
    app.revoke_unstarted_claim_root(&a.session_id,"claim-cancel",&handoff).await.unwrap();
    assert!(app.peek_android_native_stream_ticket(&ticket.stream_ticket).await.is_err());
    assert!(app.refresh_android_native_stream_ticket(&ticket.stream_ticket,None,None).await.is_err());
    assert!(app.consume_android_native_launch_token(&a.launch_token).await.is_err());
    assert!(app.peek_android_native_stream_ticket(&other_ticket.stream_ticket).await.is_ok());
    app.revoke_unstarted_claim_root(&a.session_id,"claim-cancel",&handoff).await.unwrap();
}

#[actix_web::test]
async fn claim_cancel_refuses_prepared_root_and_unknown_root_cannot_select_another() {
    let(app,_root)=fixture().await;let a=launch(&app).await;
    let(_,t)=app.consume_android_native_launch_token_with_stream_ticket(&a.launch_token).await.unwrap();
    let h=Uuid::new_v4().to_string();app.inner.android_native_session_lifecycle.write().await.get_mut(&a.session_id).unwrap().rental_binding=Some(AndroidNativeRentalBindingRecord{claim_id:"c".into(),handoff_binding_id:h.clone(),prepare_operation_id:Some("prepare_fixture_123456".into()),rental_session_id:Some("rental".into())});
    assert!(app.revoke_unstarted_claim_root(&a.session_id,"c",&h).await.is_err());
    app.revoke_unstarted_claim_root(&format!("android-native-{}",Uuid::new_v4()),"c",&h).await.unwrap();
    assert!(app.peek_android_native_stream_ticket(&t.stream_ticket).await.is_ok());
}
