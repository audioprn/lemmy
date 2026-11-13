-- audioprn: credits on posts (writer, editor, voice actor). A name is a username (u/name) or plain text.
CREATE TABLE post_credit (
    id serial PRIMARY KEY,
    post_id int NOT NULL REFERENCES post ON UPDATE CASCADE ON DELETE CASCADE,
    role text NOT NULL CHECK (role IN ('writer', 'editor', 'voice_actor')),
    name text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 100)
);

CREATE INDEX idx_post_credit_post ON post_credit (post_id);
