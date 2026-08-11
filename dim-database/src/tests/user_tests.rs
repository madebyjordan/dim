use dim_auth::generate_key;
use dim_auth::set_key_fallible;

use crate::get_conn_memory;
use crate::user;
use crate::user::Login;
use crate::user::Roles;
use crate::user::Session;
use crate::user::User;
use crate::write_tx;

pub async fn insert_user(conn: &mut crate::Transaction<'_>) -> User {
    let invite = Login::new_invite(&mut *conn).await.unwrap();
    let user = user::InsertableUser {
        username: "test".into(),
        password: "test".into(),
        roles: Roles(vec!["User".into()]),
        prefs: Default::default(),
        claimed_invite: invite,
    };

    user.insert(&mut *conn).await.unwrap()
}

pub async fn insert_many(conn: &mut crate::Transaction<'_>, n: usize) {
    for i in 0..n {
        let invite = Login::new_invite(&mut *conn).await.unwrap();
        let user = user::InsertableUser {
            username: format!("test{}", i),
            password: "test".into(),
            roles: Roles(vec!["User".into()]),
            prefs: Default::default(),
            claimed_invite: invite,
        };

        user.insert(&mut *conn).await.unwrap();
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn test_get_one() {
    let mut conn = get_conn_memory().await.unwrap().writer().lock_owned().await;
    let mut tx = write_tx(&mut conn).await.unwrap();

    let result = user::User::authenticate(&mut tx, "test".into(), "test".into()).await;
    assert!(result.is_err());

    let user = insert_user(&mut tx).await;
    assert_eq!(user.username, "test");
    let result = user::User::authenticate(&mut tx, "test".into(), "test".into())
        .await
        .unwrap();
    assert_eq!(result.username, "test".to_string());
    assert_eq!(result.roles, Roles(vec!["User".to_string()]));
}

#[tokio::test(flavor = "multi_thread")]
async fn test_get_all() {
    let mut conn = get_conn_memory().await.unwrap().writer().lock_owned().await;
    let mut tx = write_tx(&mut conn).await.unwrap();

    let result = user::User::get_all(&mut tx).await.unwrap();
    assert!(result.is_empty());

    insert_many(&mut tx, 10).await;

    let result = user::User::get_all(&mut tx).await.unwrap();
    assert_eq!(result.len(), 10);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_delete() {
    let mut conn = get_conn_memory().await.unwrap().writer().lock_owned().await;
    let mut tx = write_tx(&mut conn).await.unwrap();
    let uname = insert_user(&mut tx).await;
    let result = user::User::authenticate(&mut tx, uname.username.clone(), "test".into())
        .await
        .unwrap();
    assert_eq!(result.username, "test".to_string());

    let rows = user::User::delete(&mut tx, uname.id).await.unwrap();
    assert_eq!(rows, 1);

    let result = user::User::authenticate(&mut tx, uname.username, "test".into()).await;
    assert!(result.is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn changing_username_preserves_password_verification() {
    let mut conn = get_conn_memory().await.unwrap().writer().lock_owned().await;
    let mut tx = write_tx(&mut conn).await.unwrap();
    let user = insert_user(&mut tx).await;

    User::set_username(&mut tx, user.id, "renamed".into())
        .await
        .unwrap();

    let authenticated = User::authenticate(&mut tx, "renamed".into(), "test".into())
        .await
        .unwrap();
    assert_eq!(authenticated.id, user.id);
}

#[tokio::test(flavor = "multi_thread")]
async fn new_passwords_use_argon2id_with_random_salts() {
    let mut conn = get_conn_memory().await.unwrap().writer().lock_owned().await;
    let mut tx = write_tx(&mut conn).await.unwrap();
    let first = insert_user(&mut tx).await;
    let first_hash: String = sqlx::query_scalar("SELECT password FROM users WHERE id = ?")
        .bind(first.id.get())
        .fetch_one(&mut tx)
        .await
        .unwrap();
    let invite = Login::new_invite(&mut tx).await.unwrap();
    let second = user::InsertableUser {
        username: "second".into(),
        password: "test".into(),
        roles: Roles(vec!["User".into()]),
        prefs: Default::default(),
        claimed_invite: invite,
    }
    .insert(&mut tx)
    .await
    .unwrap();
    let second_hash: String = sqlx::query_scalar("SELECT password FROM users WHERE id = ?")
        .bind(second.id.get())
        .fetch_one(&mut tx)
        .await
        .unwrap();
    assert!(first_hash.starts_with("$argon2id$v=19$"));
    assert!(second_hash.starts_with("$argon2id$v=19$"));
    assert_ne!(first_hash, second_hash);
}

#[tokio::test(flavor = "multi_thread")]
async fn legacy_pbkdf2_migrates_on_successful_login_and_survives_rename() {
    let mut conn = get_conn_memory().await.unwrap().writer().lock_owned().await;
    let mut tx = write_tx(&mut conn).await.unwrap();
    let invite = Login::new_invite(&mut tx).await.unwrap();
    let legacy = user::hash("legacy-name".into(), "password".into());
    sqlx::query("INSERT INTO users (username, password, password_salt, prefs, claimed_invite, roles) VALUES (?, ?, ?, '{}', ?, '[]')")
        .bind("legacy-name")
        .bind(legacy)
        .bind("legacy-name")
        .bind(invite)
        .execute(&mut tx)
        .await
        .unwrap();
    let id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'legacy-name'")
        .fetch_one(&mut tx)
        .await
        .unwrap();
    User::set_username(&mut tx, user::UserID(id), "renamed-legacy".into())
        .await
        .unwrap();
    User::authenticate(&mut tx, "renamed-legacy".into(), "password".into())
        .await
        .unwrap();
    let upgraded: String = sqlx::query_scalar("SELECT password FROM users WHERE id = ?")
        .bind(id)
        .fetch_one(&mut tx)
        .await
        .unwrap();
    assert!(upgraded.starts_with("$argon2id$v=19$"));
    assert!(
        User::authenticate(&mut tx, "renamed-legacy".into(), "wrong".into())
            .await
            .is_err()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn sessions_expire_and_can_be_revoked() {
    let mut conn = get_conn_memory().await.unwrap().writer().lock_owned().await;
    let mut tx = write_tx(&mut conn).await.unwrap();
    let user = insert_user(&mut tx).await;
    let (_, token) = Session::create(&mut tx, user.id, 60).await.unwrap();
    assert_eq!(
        Session::verify(&mut tx, &token).await.unwrap().user_id,
        user.id
    );
    assert_eq!(Session::revoke_user(&mut tx, user.id).await.unwrap(), 1);
    assert!(Session::verify(&mut tx, &token).await.is_err());
    let (_, expired) = Session::create(&mut tx, user.id, 0).await.unwrap();
    assert!(Session::verify(&mut tx, &expired).await.is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn test_invites() {
    let mut conn = get_conn_memory().await.unwrap().writer().lock_owned().await;
    let mut tx = write_tx(&mut conn).await.unwrap();

    let result = user::Login::get_all_invites(&mut tx).await.unwrap();
    assert!(result.is_empty());

    let invite = user::Login::new_invite(&mut tx).await.unwrap();
    let result = user::Login::get_all_invites(&mut tx).await.unwrap();
    assert_eq!(&result, &[invite.clone()]);

    let result = user::Login {
        invite_token: Some(invite.clone()),
        ..Default::default()
    }
    .invite_token_valid(&mut tx)
    .await
    .unwrap();
    assert!(result);

    let result = user::Login {
        invite_token: Some("TESTTESTTEST".into()),
        ..Default::default()
    }
    .invite_token_valid(&mut tx)
    .await
    .unwrap();
    assert!(!result);

    let result = user::Login {
        invite_token: Some(invite.clone()),
        ..Default::default()
    }
    .invalidate_token(&mut tx)
    .await
    .unwrap();
    assert_eq!(result, 1);

    let result = user::Login::get_all_invites(&mut tx).await.unwrap();
    assert!(result.is_empty());

    let result = user::Login {
        invite_token: Some(invite),
        ..Default::default()
    }
    .invalidate_token(&mut tx)
    .await
    .unwrap();
    assert_eq!(result, 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_cookie_encoding() {
    let _ = set_key_fallible(generate_key());
    let mut conn = get_conn_memory().await.unwrap().writer().lock_owned().await;
    let mut tx = write_tx(&mut conn).await.unwrap();

    let user = insert_user(&mut tx).await;
    let token = Login::create_cookie(user.id);
    let token2 = Login::create_cookie(user.id);
    assert_ne!(token, token2);
    let uid = Login::verify_cookie(token).unwrap();
    assert_eq!(uid, user.id);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_invalid_cookie() {
    let _ = set_key_fallible(generate_key());
    let res = Login::verify_cookie(String::new());
    assert!(res.is_err());
    let res = Login::verify_cookie(String::from("ansd9uid89as"));
    assert!(res.is_err());
    let res = Login::verify_cookie(String::from("bXl1c2VyaWQ="));
    assert!(res.is_err());
}
