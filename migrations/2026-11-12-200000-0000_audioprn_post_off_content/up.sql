-- audioprn: posts their creator keeps off the Content page (they still show in their community).
CREATE TABLE post_off_content (
    post_id int PRIMARY KEY REFERENCES post ON UPDATE CASCADE ON DELETE CASCADE
);
