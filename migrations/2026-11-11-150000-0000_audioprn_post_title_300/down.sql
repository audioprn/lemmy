UPDATE
    post
SET
    name = left(name, 200)
WHERE
    char_length(name) > 200;

UPDATE
    post_report
SET
    original_post_name = left(original_post_name, 200)
WHERE
    char_length(original_post_name) > 200;

ALTER TABLE post
    ALTER COLUMN name TYPE varchar(200);

ALTER TABLE post_report
    ALTER COLUMN original_post_name TYPE varchar(200);
