use crate::DatabaseError;
use std::collections::HashMap;
use std::num::NonZeroU32;
use std::time::SystemTime;

use dim_auth::user_cookie_decode;
use dim_auth::user_cookie_generate;
use dim_auth::AuthError;
use serde::Deserialize;
use serde::Serialize;

use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::{Algorithm, Argon2, Params, Version};
use ring::digest;
use ring::pbkdf2;
use sqlx::Decode;
use sqlx::Encode;

static PBKDF2_ALG: pbkdf2::Algorithm = pbkdf2::PBKDF2_HMAC_SHA256;
const CREDENTIAL_LEN: usize = digest::SHA256_OUTPUT_LEN;
const HASH_ROUNDS: NonZeroU32 = unsafe { NonZeroU32::new_unchecked(1_000) };
const ARGON_MEMORY_KIB: u32 = 19_456;
const ARGON_ITERATIONS: u32 = 2;
const ARGON_PARALLELISM: u32 = 1;

pub type Credential = [u8; CREDENTIAL_LEN];

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type)]
pub enum Theme {
    Light,
    Dark,
    Black,
}

pub fn default_theme() -> Theme {
    Theme::Dark
}

pub fn default_true() -> bool {
    true
}

pub fn default_false() -> bool {
    false
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DefaultVideoQuality {
    /// Represents DirectPlay quality
    DirectPlay,
    /// Represents a default video quality made up of resolution and bitrate.
    Resolution(u64, u64),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserSettings {
    /// Theme of the app
    #[serde(default = "default_theme")]
    theme: Theme,
    /// Defines whether the sidebar should be collapsed or not
    #[serde(default = "default_false")]
    is_sidebar_compact: bool,
    #[serde(default = "default_true")]
    show_card_names: bool,
    /// If this contains a string then the filebrowser/explorer will default to this path instead of `/`.
    filebrowser_default_path: Option<String>,
    #[serde(default = "default_true")]
    filebrowser_list_view: bool,
    /// If a file has subtitles then the subtitles with this language will be selected.
    default_subtitle_language: Option<String>,
    /// If a file has audio then the audio track with this language will be selected, otherwise the first one.
    default_audio_language: Option<String>,
    /// Represents the default video quality for user.
    pub default_video_quality: DefaultVideoQuality,
    /// Any other external args.
    #[serde(default)]
    external_args: HashMap<String, String>,
    /// Whether hovercards are hidden or not
    #[serde(default)]
    show_hovercards: bool,
    /// Whether to auto play next video
    enable_autoplay: bool,
}

impl<DB: sqlx::Database> sqlx::Type<DB> for UserSettings
where
    Vec<u8>: sqlx::Type<DB>,
{
    fn type_info() -> DB::TypeInfo {
        <Vec<u8> as sqlx::Type<DB>>::type_info()
    }
}

impl<'r, DB: sqlx::Database> Decode<'r, DB> for UserSettings
where
    &'r [u8]: Decode<'r, DB>,
{
    fn decode(
        value: <DB as sqlx::database::HasValueRef<'r>>::ValueRef,
    ) -> Result<Self, sqlx::error::BoxDynError> {
        let value = <&[u8] as Decode<DB>>::decode(value)?;
        Ok(serde_json::from_slice(value).unwrap_or_default())
    }
}

impl<'q, DB: sqlx::Database> Encode<'q, DB> for UserSettings
where
    Vec<u8>: Encode<'q, DB>,
{
    fn encode_by_ref(
        &self,
        buf: &mut <DB as sqlx::database::HasArguments<'q>>::ArgumentBuffer,
    ) -> sqlx::encode::IsNull {
        let val = serde_json::to_vec(self).unwrap_or_default();
        <Vec<u8> as Encode<DB>>::encode(val, buf)
    }
}

impl Default for UserSettings {
    fn default() -> Self {
        Self {
            theme: Theme::Dark,
            is_sidebar_compact: false,
            show_card_names: true,
            filebrowser_default_path: None,
            filebrowser_list_view: true,
            default_subtitle_language: Some("english".into()),
            default_audio_language: Some("english".into()),
            external_args: HashMap::new(),
            show_hovercards: true,
            default_video_quality: DefaultVideoQuality::DirectPlay,
            enable_autoplay: true,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq)]
pub enum Role {
    Owner,
    User,
}

#[derive(Copy, Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize, sqlx::Type)]
#[sqlx(transparent)]
pub struct UserID(pub(crate) i64);

impl UserID {
    pub fn get(self) -> i64 {
        self.0
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(transparent)]
pub struct Roles(pub Vec<String>);

impl<DB: sqlx::Database> sqlx::Type<DB> for Roles
where
    String: sqlx::Type<DB>,
{
    fn type_info() -> DB::TypeInfo {
        <String as sqlx::Type<DB>>::type_info()
    }
}

impl<'r, DB: sqlx::Database> Decode<'r, DB> for Roles
where
    &'r str: Decode<'r, DB>,
{
    fn decode(
        value: <DB as sqlx::database::HasValueRef<'r>>::ValueRef,
    ) -> Result<Self, sqlx::error::BoxDynError> {
        let value = <&str as Decode<DB>>::decode(value)?;
        Ok(serde_json::from_str(value).unwrap_or_default())
    }
}

impl<'q, DB: sqlx::Database> Encode<'q, DB> for Roles
where
    String: Encode<'q, DB>,
{
    fn encode_by_ref(
        &self,
        buf: &mut <DB as sqlx::database::HasArguments<'q>>::ArgumentBuffer,
    ) -> sqlx::encode::IsNull {
        let val = serde_json::to_string(self).unwrap_or_default();
        <String as Encode<DB>>::encode(val, buf)
    }
}

#[derive(Debug, Clone)]
pub struct User {
    pub id: UserID,
    pub username: String,
    pub roles: Roles,
    pub prefs: UserSettings,
    pub picture: Option<i64>,
}

impl User {
    /// Method gets all entries from the table users.
    pub async fn get_all(conn: &mut crate::Transaction<'_>) -> Result<Vec<Self>, DatabaseError> {
        Ok(
            sqlx::query!(
                r#"SELECT id as "id: UserID", username, roles as "roles: Roles", prefs as "prefs: UserSettings", picture FROM users"#
            )
            .fetch_all(&mut *conn)
            .await?
            .into_iter()
            .map(|user| Self {
                id: user.id,
                username: user.username,
                roles: user.roles,
                prefs: user.prefs,
                picture: user.picture,
            })
            .collect(),
        )
    }

    pub async fn get_by_id(
        conn: &mut crate::Transaction<'_>,
        uid: UserID,
    ) -> Result<Self, DatabaseError> {
        Ok(sqlx::query!(
            r#"SELECT id as "id: UserID", username, roles as "roles: Roles", prefs as "prefs: UserSettings", picture from users
                WHERE id = ?"#,
            uid
        )
        .fetch_one(&mut *conn)
        .await
        .map(|u| Self {
            id: u.id,
            username: u.username,
            roles: u.roles,
            prefs: u.prefs,
            picture: u.picture,
        })?)
    }

    pub async fn get(
        conn: &mut crate::Transaction<'_>,
        username: &str,
    ) -> Result<Self, DatabaseError> {
        Ok(sqlx::query!(
            r#"SELECT id as "id: UserID", username, roles as "roles: Roles", prefs as "prefs: UserSettings", picture from users
                WHERE username = ?"#,
            username
        )
        .fetch_one(&mut *conn)
        .await
        .map(|u| Self {
            id: u.id,
            username: u.username,
            roles: u.roles,
            prefs: u.prefs,
            picture: u.picture,
        })?)
    }

    /// Method gets one entry from the table users based on the username supplied and password.
    ///
    /// # Arguments
    /// * `uname` - username we wish to target and delete
    /// * `pw_hash` - hash of the password for the user we are trying to access
    pub async fn authenticate(
        conn: &mut crate::Transaction<'_>,
        uname: String,
        pw: String,
    ) -> Result<Self, DatabaseError> {
        let record = sqlx::query!(
            r#"SELECT id, password, password_salt FROM users WHERE username = ?"#,
            uname
        )
        .fetch_one(&mut *conn)
        .await?;
        let verification = verify_password(
            record.password_salt.as_deref().unwrap_or(""),
            &record.password,
            &pw,
        );
        if verification == PasswordVerification::Invalid {
            return Err(sqlx::Error::RowNotFound.into());
        }
        if verification == PasswordVerification::Legacy {
            let upgraded = hash_password(&pw)?;
            sqlx::query!(
                "UPDATE users SET password = $1, password_salt = NULL WHERE id = $2",
                upgraded,
                record.id
            )
            .execute(&mut *conn)
            .await?;
        }
        let user = sqlx::query!(
            r#"SELECT id as "id: UserID", username, roles as "roles: Roles", prefs as "prefs: UserSettings", picture FROM users WHERE id = ?"#,
            record.id,
        )
        .fetch_one(&mut *conn)
        .await?;

        Ok(Self {
            id: user.id,
            username: user.username,
            roles: user.roles,
            prefs: user.prefs,
            picture: user.picture,
        })
    }

    /// Method gets users password from the table users based on the user
    ///
    /// # Arguments
    /// * `conn` - DBTransaction
    pub async fn get_pass(
        &self,
        conn: &mut crate::Transaction<'_>,
    ) -> Result<String, DatabaseError> {
        let pass = sqlx::query!("SELECT password FROM users WHERE id = ?", self.id,)
            .fetch_one(&mut *conn)
            .await
            .map(|r| r.password)?;

        Ok(pass)
    }

    /// Method deletes a entry from the table users and returns the number of rows deleted.
    /// NOTE: Return should always be 1
    ///
    /// # Arguments
    /// * `uname` - username we wish to target and delete
    pub async fn delete(
        conn: &mut crate::Transaction<'_>,
        uid: UserID,
    ) -> Result<usize, DatabaseError> {
        Ok(sqlx::query!("DELETE FROM users WHERE id = ?", uid)
            .execute(&mut *conn)
            .await?
            .rows_affected() as usize)
    }

    /// Method resets the password for a user to a new password.
    ///
    /// # Arguments
    /// * `&` - db &ection
    /// * `password` - new password.
    pub async fn set_password(
        &self,
        conn: &mut crate::Transaction<'_>,
        password: String,
    ) -> Result<usize, DatabaseError> {
        let hash = hash_password(&password)?;

        Ok(sqlx::query!(
            "UPDATE users SET password = $1, password_salt = NULL WHERE username = ?2",
            hash,
            self.username
        )
        .execute(&mut *conn)
        .await?
        .rows_affected() as usize)
    }

    pub async fn set_username(
        conn: &mut crate::Transaction<'_>,
        user_id: UserID,
        new_username: String,
    ) -> Result<usize, DatabaseError> {
        Ok(sqlx::query!(
            "UPDATE users SET username = $1 WHERE users.id = ?2",
            new_username,
            user_id
        )
        .execute(&mut *conn)
        .await?
        .rows_affected() as usize)
    }

    pub async fn set_picture(
        conn: &mut crate::Transaction<'_>,
        uid: UserID,
        asset_id: i64,
    ) -> Result<usize, DatabaseError> {
        Ok(sqlx::query!(
            "UPDATE users SET picture = $1 WHERE users.id = ?2",
            asset_id,
            uid
        )
        .execute(&mut *conn)
        .await?
        .rows_affected() as usize)
    }

    pub async fn clear_picture(
        conn: &mut crate::Transaction<'_>,
        uid: UserID,
    ) -> Result<usize, DatabaseError> {
        Ok(
            sqlx::query!("UPDATE users SET picture = NULL WHERE users.id = ?", uid)
                .execute(&mut *conn)
                .await?
                .rows_affected() as usize,
        )
    }

    pub fn has_role(&self, role: &str) -> bool {
        self.roles.0.contains(&role.to_string())
    }

    pub fn roles(&self) -> Roles {
        self.roles.clone()
    }
}

#[derive(Deserialize)]
pub struct InsertableUser {
    pub username: String,
    pub password: String,
    pub roles: Roles,
    pub prefs: UserSettings,
    pub claimed_invite: String,
}

impl InsertableUser {
    /// Method consumes a InsertableUser object and inserts the values under it into database
    /// table as a new user
    pub async fn insert(self, conn: &mut crate::Transaction<'_>) -> Result<User, DatabaseError> {
        let Self {
            username,
            password,
            roles,
            prefs,
            claimed_invite,
        } = self;

        let password = hash_password(&password)?;

        let user = sqlx::query_as!(
            User,
            r#"INSERT INTO users (username, password, password_salt, prefs, claimed_invite, roles) VALUES ($1, $2, NULL, $3, $4, $5) returning id as "id: UserID",username,roles as "roles: Roles",prefs as "prefs: UserSettings",picture"#,
            username,
            password,
            prefs,
            claimed_invite,
            roles
        ).fetch_one(&mut *conn)
        .await?;
        Ok(user)
    }
}

#[derive(Deserialize)]
pub struct UpdateableUser {
    pub prefs: Option<UserSettings>,
}

impl UpdateableUser {
    pub async fn update(
        &self,
        conn: &mut crate::Transaction<'_>,
        user: UserID,
    ) -> Result<usize, DatabaseError> {
        if let Some(prefs) = &self.prefs {
            return Ok(sqlx::query!(
                "UPDATE users SET prefs = $1 WHERE users.id = ?",
                prefs,
                user
            )
            .execute(&mut *conn)
            .await?
            .rows_affected() as usize);
        }

        Ok(0)
    }
}

#[derive(Deserialize, Default)]
pub struct Login {
    pub username: String,
    pub password: String,
    pub invite_token: Option<String>,
}

impl Login {
    /// Will return whether the token is valid and hasnt been claimed yet.
    pub async fn invite_token_valid(
        &self,
        conn: &mut crate::Transaction<'_>,
    ) -> Result<bool, DatabaseError> {
        let tok = match &self.invite_token {
            None => return Ok(false),
            Some(t) => t,
        };

        Ok(sqlx::query!(
            "SELECT id FROM invites
                          WHERE id NOT IN (
                              SELECT claimed_invite FROM users
                          )
                          AND id = ?",
            tok
        )
        .fetch_optional(&mut *conn)
        .await?
        .is_some())
    }

    pub async fn invalidate_token(
        &self,
        conn: &mut crate::Transaction<'_>,
    ) -> Result<usize, DatabaseError> {
        if let Some(tok) = &self.invite_token {
            Ok(sqlx::query!("DELETE FROM invites WHERE id = ?", tok)
                .execute(&mut *conn)
                .await?
                .rows_affected() as usize)
        } else {
            Ok(0)
        }
    }

    pub async fn new_invite(conn: &mut crate::Transaction<'_>) -> Result<String, DatabaseError> {
        let ts = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let token = uuid::Uuid::new_v4().to_hyphenated().to_string();
        let _ = sqlx::query!(
            "INSERT INTO invites (id, date_added) VALUES ($1, $2)",
            token,
            ts
        )
        .execute(&mut *conn)
        .await?;

        Ok(token)
    }

    pub async fn get_all_invites(
        conn: &mut crate::Transaction<'_>,
    ) -> Result<Vec<String>, DatabaseError> {
        Ok(sqlx::query!("SELECT id from invites")
            .fetch_all(&mut *conn)
            .await?
            .into_iter()
            .map(|t| t.id)
            .collect())
    }

    pub async fn delete_token(
        conn: &mut crate::Transaction<'_>,
        token: String,
    ) -> Result<usize, DatabaseError> {
        Ok(sqlx::query!(
            "DELETE FROM invites
                WHERE id NOT IN (
                    SELECT claimed_invite FROM users
                ) AND id = ?",
            token
        )
        .execute(&mut *conn)
        .await?
        .rows_affected() as usize)
    }

    pub fn create_cookie(id: UserID) -> String {
        user_cookie_generate(id.0)
    }

    pub fn verify_cookie(cookie: String) -> Result<UserID, AuthError> {
        Ok(UserID(user_cookie_decode(cookie)?))
    }
}

pub fn hash(salt: String, s: String) -> String {
    let mut to_store: Credential = [0u8; CREDENTIAL_LEN];
    pbkdf2::derive(
        PBKDF2_ALG,
        HASH_ROUNDS,
        &salt.as_bytes(),
        s.as_bytes(),
        &mut to_store,
    );
    base64::encode(&to_store)
}

pub fn verify(salt: String, password: String, attempted_password: String) -> bool {
    verify_password(&salt, &password, &attempted_password) != PasswordVerification::Invalid
}

fn argon2() -> Argon2<'static> {
    Argon2::new(
        Algorithm::Argon2id,
        Version::V0x13,
        Params::new(ARGON_MEMORY_KIB, ARGON_ITERATIONS, ARGON_PARALLELISM, None)
            .expect("fixed Argon2 parameters are valid"),
    )
}

pub fn hash_password(password: &str) -> Result<String, DatabaseError> {
    let salt = SaltString::generate(&mut argon2::password_hash::rand_core::OsRng);
    argon2()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|error| sqlx::Error::Protocol(format!("password hashing failed: {error}")).into())
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum PasswordVerification {
    Invalid,
    Legacy,
    Argon2id,
}

pub fn verify_password(salt: &str, stored: &str, attempted: &str) -> PasswordVerification {
    if stored.starts_with("$argon2id$") {
        return PasswordHash::new(stored)
            .ok()
            .filter(|parsed| {
                argon2()
                    .verify_password(attempted.as_bytes(), parsed)
                    .is_ok()
            })
            .map(|_| PasswordVerification::Argon2id)
            .unwrap_or(PasswordVerification::Invalid);
    }

    let Ok(real_pwd) = base64::decode(stored) else {
        return PasswordVerification::Invalid;
    };

    if pbkdf2::verify(
        PBKDF2_ALG,
        HASH_ROUNDS,
        salt.as_bytes(),
        attempted.as_bytes(),
        real_pwd.as_slice(),
    )
    .is_ok()
    {
        PasswordVerification::Legacy
    } else {
        PasswordVerification::Invalid
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Session {
    pub id: String,
    pub user_id: UserID,
    pub expires_at: i64,
}

impl Session {
    pub async fn create(
        conn: &mut crate::Transaction<'_>,
        user_id: UserID,
        ttl_seconds: u64,
    ) -> Result<(Self, String), DatabaseError> {
        let now = unix_timestamp();
        let ttl = i64::try_from(ttl_seconds).unwrap_or(i64::MAX);
        let expires_at = now.saturating_add(ttl);
        let revoked_before = now.saturating_sub(24 * 60 * 60);
        sqlx::query!(
            "DELETE FROM auth_sessions WHERE expires_at <= $1 OR (revoked_at IS NOT NULL AND revoked_at <= $2)",
            now,
            revoked_before
        )
        .execute(&mut *conn)
        .await?;
        let id = uuid::Uuid::new_v4().to_hyphenated().to_string();
        // Two independently generated UUIDv4 values provide 244 random bits without adding a
        // second RNG dependency. Only the SHA-256 digest is retained server-side.
        let token = format!(
            "{}.{}",
            uuid::Uuid::new_v4().to_simple(),
            uuid::Uuid::new_v4().to_simple()
        );
        let token_hash = session_token_hash(&token);
        sqlx::query!(
            "INSERT INTO auth_sessions (id, user_id, token_hash, created_at, expires_at) VALUES ($1, $2, $3, $4, $5)",
            id,
            user_id,
            token_hash,
            now,
            expires_at
        )
        .execute(&mut *conn)
        .await?;
        Ok((
            Self {
                id,
                user_id,
                expires_at,
            },
            token,
        ))
    }

    pub async fn verify(
        conn: &mut crate::Transaction<'_>,
        token: &str,
    ) -> Result<Self, DatabaseError> {
        let token_hash = session_token_hash(token);
        let now = unix_timestamp();
        let row = sqlx::query!(
            "SELECT id, user_id, expires_at FROM auth_sessions WHERE token_hash = $1 AND revoked_at IS NULL AND expires_at > $2",
            token_hash,
            now
        )
        .fetch_one(&mut *conn)
        .await?;
        Ok(Self {
            id: row.id,
            user_id: UserID(row.user_id),
            expires_at: row.expires_at,
        })
    }

    pub async fn revoke_user(
        conn: &mut crate::Transaction<'_>,
        user_id: UserID,
    ) -> Result<usize, DatabaseError> {
        let now = unix_timestamp();
        Ok(sqlx::query!(
            "UPDATE auth_sessions SET revoked_at = $1 WHERE user_id = $2 AND revoked_at IS NULL",
            now,
            user_id
        )
        .execute(&mut *conn)
        .await?
        .rows_affected() as usize)
    }

    pub async fn revoke(
        conn: &mut crate::Transaction<'_>,
        session_id: &str,
    ) -> Result<usize, DatabaseError> {
        let now = unix_timestamp();
        Ok(sqlx::query!(
            "UPDATE auth_sessions SET revoked_at = $1 WHERE id = $2 AND revoked_at IS NULL",
            now,
            session_id
        )
        .execute(&mut *conn)
        .await?
        .rows_affected() as usize)
    }
}

fn unix_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn session_token_hash(token: &str) -> String {
    base64::encode(digest::digest(&digest::SHA256, token.as_bytes()).as_ref())
}
