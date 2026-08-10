# Media provider identity follow-up

Milestone 0 constrains title-based media reuse to records with the same library and media type.
This prevents the confirmed cross-library and cross-type sharing defect without requiring a schema
redesign, but titles are not stable identities: remakes and distinct releases can still have the
same title inside one library.

A later milestone should persist provider identity on top-level media records. The schema should
store at least `provider` and `provider_id`, enforce uniqueness within the intended library scope,
and define how manual matches, provider changes, local-only metadata, and existing rows are
backfilled. Scanner matching should prefer that identity and use title/year only as an explicit
fallback for records without provider data.
