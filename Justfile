default:
    just --list

# Bump the Smuggler version and push a release commit and tag to origin.
release part="patch":
    python3 scripts/release.py {{quote(part)}}
