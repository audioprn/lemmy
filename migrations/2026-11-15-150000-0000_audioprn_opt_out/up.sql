-- audioprn: Reddit users who asked not to appear here (takedown requests, then admins' opt-out
-- list). Adding a name removes the imported posts that mention it; new posts can't mention it.
CREATE TABLE takedown_request (
    id serial PRIMARY KEY,
    reddit_name text NOT NULL,
    contact text NOT NULL DEFAULT '' CHECK (char_length(contact) <= 200),
    message text NOT NULL DEFAULT '' CHECK (char_length(message) <= 2000),
    published_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE opt_out (
    id serial PRIMARY KEY,
    reddit_name text NOT NULL,
    admin_id int REFERENCES person ON UPDATE CASCADE ON DELETE SET NULL,
    posts_removed int NOT NULL DEFAULT 0,
    published_at timestamptz NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX idx_opt_out_name ON opt_out (lower(reddit_name));
