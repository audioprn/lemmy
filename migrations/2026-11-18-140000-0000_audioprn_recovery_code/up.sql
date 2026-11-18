-- audioprn: no email (user, 2026-11-18). A listener's recovery code (only its hash), shown once,
-- to set a new password without email. Creators and admins ask an admin instead.
CREATE TABLE recovery_code (
    local_user_id int PRIMARY KEY REFERENCES local_user ON UPDATE CASCADE ON DELETE CASCADE,
    code_hash text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);
