-- audioprn: a space between tags that touch in titles: "[F4M][Romance]" becomes
-- "[F4M] [Romance]" (new and edited posts get this too).
UPDATE
    post
SET
    name = replace(name, '][', '] [')
WHERE
    name LIKE '%][%';

