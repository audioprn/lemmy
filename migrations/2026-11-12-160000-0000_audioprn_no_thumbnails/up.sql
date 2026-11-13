-- audioprn: no images on the site, so posts never keep a link thumbnail.
CREATE FUNCTION audioprn_no_thumbnail ()
    RETURNS TRIGGER
    LANGUAGE plpgsql
    AS $$
BEGIN
    NEW.thumbnail_url := NULL;
    RETURN NEW;
END
$$;

CREATE TRIGGER audioprn_no_thumbnail
    BEFORE INSERT OR UPDATE OF thumbnail_url ON post
    FOR EACH ROW
    EXECUTE FUNCTION audioprn_no_thumbnail ();

UPDATE
    post
SET
    thumbnail_url = NULL
WHERE
    thumbnail_url IS NOT NULL;
