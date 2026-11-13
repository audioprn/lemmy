-- audioprn: admin news flashes at the top of the Content and My audioprn left column.
-- Up to 300 characters with each link counted as 23 (checked in crates/audioprn).
CREATE TABLE news_flash (
    id serial PRIMARY KEY,
    body text NOT NULL CHECK (char_length(body) BETWEEN 1 AND 3000),
    published_at timestamptz NOT NULL DEFAULT now()
);
