-- Passwords in existing Dim databases are PBKDF2-derived using the username as
-- the salt. Preserve that salt separately so a username change does not make
-- the stored credential unverifiable.
ALTER TABLE users ADD COLUMN password_salt TEXT;
UPDATE users SET password_salt = username WHERE password_salt IS NULL;

