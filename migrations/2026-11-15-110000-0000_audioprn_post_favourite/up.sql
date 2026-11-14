-- audioprn: favourite posts (♥), separate from Lemmy's Save (to listen to later). Listed on My
-- audioprn (/my/favourites) and as ♥ Favourites on creators' masterlists.
CREATE TABLE post_favourite (
    person_id int NOT NULL REFERENCES person ON UPDATE CASCADE ON DELETE CASCADE,
    post_id int NOT NULL REFERENCES post ON UPDATE CASCADE ON DELETE CASCADE,
    published_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (person_id, post_id)
);

CREATE INDEX idx_post_favourite_post ON post_favourite (post_id);
