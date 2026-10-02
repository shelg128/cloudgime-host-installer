//! Accounting provenance only, set at the original raw-video sender. No grant,
//! peer/socket identity or permission to duplicate travels in this marker.
use std::future::Future;
tokio::task_local! {static ORIGINAL: (u16,u32);}
pub(super) async fn within<F:Future>(seq:u16,ts:u32,send:F)->F::Output{ORIGINAL.scope((seq,ts),send).await}
pub(super) fn matches(seq:u16,ts:u32)->bool{ORIGINAL.try_with(|v|*v==(seq,ts)).unwrap_or(false)}
#[cfg(test)]mod tests{
 use super::*;
 #[tokio::test]async fn identity_and_lifetime_are_exact(){assert!(!matches(1,2));within(1,2,async{assert!(matches(1,2));assert!(!matches(2,2));assert!(!matches(1,3));}).await;assert!(!matches(1,2));}
 #[tokio::test]async fn nack_spawn_does_not_inherit_original_marker(){within(1,2,async{assert!(tokio::spawn(async{!matches(1,2)}).await.unwrap());assert!(matches(1,2));}).await;}
 #[tokio::test]async fn nested_scope_restores_outer_and_cancel_clears(){within(1,2,async{within(3,4,async{assert!(matches(3,4));assert!(!matches(1,2));}).await;assert!(matches(1,2));}).await;assert!(!matches(3,4));assert!(tokio::time::timeout(std::time::Duration::from_millis(1),within(9,9,std::future::pending::<()>())).await.is_err());assert!(!matches(9,9));}
}
