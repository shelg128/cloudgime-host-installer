use super::*;
#[actix_web::test]
async fn claim_cancel_revokes_only_exact_unprepared_root() {
 let(app,_root)=fixture().await;let a=launch(&app).await;
 let(_,t)=app.consume_android_native_launch_token_with_stream_ticket(&a.launch_token).await.unwrap();
 let h=Uuid::new_v4().to_string();
 app.inner.android_native_session_lifecycle.write().await.get_mut(&a.session_id).unwrap().rental_binding=Some(AndroidNativeRentalBindingRecord{claim_id:"claim".into(),handoff_binding_id:h.clone(),prepare_operation_id:None,rental_session_id:None});
 assert!(app.revoke_unstarted_claim_root(&a.session_id,"other",&h).await.is_err());
 assert!(app.peek_android_native_stream_ticket(&t.stream_ticket).await.is_ok());
 app.revoke_unstarted_claim_root(&a.session_id,"claim",&h).await.unwrap();
 assert!(app.peek_android_native_stream_ticket(&t.stream_ticket).await.is_err());
 app.revoke_unstarted_claim_root(&a.session_id,"claim",&h).await.unwrap();
}
#[actix_web::test]
async fn claim_cancel_refuses_prepared_root() {
 let(app,_root)=fixture().await;let a=launch(&app).await;
 let(_,t)=app.consume_android_native_launch_token_with_stream_ticket(&a.launch_token).await.unwrap();
 let h=Uuid::new_v4().to_string();
 app.inner.android_native_session_lifecycle.write().await.get_mut(&a.session_id).unwrap().rental_binding=Some(AndroidNativeRentalBindingRecord{claim_id:"claim".into(),handoff_binding_id:h.clone(),prepare_operation_id:Some("prepare_fixture_123456".into()),rental_session_id:Some("rental".into())});
 assert!(app.revoke_unstarted_claim_root(&a.session_id,"claim",&h).await.is_err());
 app.revoke_unstarted_claim_root(&format!("android-native-{}",Uuid::new_v4()),"claim",&h).await.unwrap();
 assert!(app.peek_android_native_stream_ticket(&t.stream_ticket).await.is_ok());
}
