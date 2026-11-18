-- audioprn: a post's own cash box text, when its creator changed it on the post form (otherwise their
-- default from Settings shows). Empty: no cash box on this post.
CREATE TABLE post_cash_box (
    post_id int PRIMARY KEY REFERENCES post ON UPDATE CASCADE ON DELETE CASCADE,
    text text NOT NULL
);

