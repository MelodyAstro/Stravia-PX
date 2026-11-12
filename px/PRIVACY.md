# Public-source privacy

This repository contains reusable code and example configuration. Keep actual server domains/IPs, SSH endpoints, personal paths, account identifiers, deployment inventories, backup receipts and credentials outside Git. Use reserved example domains and documentation IP ranges in public instructions.

Private operational data belongs outside the checkout or in ignored `private/`, `backups/`, `evidence/` or `px/local/` directories. Ignore rules do not remove previously tracked material.

Management helpers read the actual site origin from `STRINOVA_ADMIN_ORIGIN`, the optional loopback port from `STRINOVA_ADMIN_PORT`, and the private access-file path from `STRINOVA_ACCESS_FILE`. Public transport smoke tests use `STRINOVA_PUBLIC_HOST` or an explicit `--host`; configure these locally, never commit their real values.

Run `python3 px/ops/check-public-source.py` before pushing. CI checks public maintenance files for unreviewed hosts, deployment IP addresses, personal paths/emails and private material; the release workflow repeats the check before publishing. The checker reports filenames and categories, never matched values. Its host allowlist covers public upstream documentation and provider APIs; review additions explicitly.

For an exposure, preserve an access-controlled recovery copy, remove the values from every affected branch and historical path, inspect artifacts/logs/release assets, then regenerate clean source-bound packages. Coordinate fresh checkouts after history rewrites so an old clone cannot reintroduce removed history. GitHub may retain cached commit views or external copies after refs change; support and clone/fork owners control those remaining copies.

Production test outcomes should be kept privately. Public release notes can describe reproducible synthetic tests and general limitations without identifying accounts or deployment addresses.
