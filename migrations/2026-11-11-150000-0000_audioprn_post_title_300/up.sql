-- audioprn: post titles can be up to 300 characters, emoji included (spec).
ALTER TABLE post
    ALTER COLUMN name TYPE varchar(300);

ALTER TABLE post_report
    ALTER COLUMN original_post_name TYPE varchar(300);
