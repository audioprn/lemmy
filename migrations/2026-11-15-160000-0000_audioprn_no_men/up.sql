-- audioprn: sapphic posts marked "No comments from men are welcome on this post" (always kept off Content).
ALTER TABLE post_off_content
    ADD COLUMN no_men boolean NOT NULL DEFAULT FALSE;

