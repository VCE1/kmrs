# kmrs

[![CI](https://github.com/kmworks/kmrs/actions/workflows/ci.yml/badge.svg)](https://github.com/kmworks/kmrs/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/kmworks/kmrs)](https://github.com/kmworks/kmrs/releases/latest)
[![Docker image](https://img.shields.io/badge/ghcr.io-kmworks%2Fkmrs-blue)](https://github.com/kmworks/kmrs/pkgs/container/kmrs)
[![License: MIT](https://img.shields.io/github/license/kmworks/kmrs)](LICENSE)

**[Documentation](https://kmworks.date/server/)** · [Installation](https://kmworks.date/server/installation) · [Configuration](https://kmworks.date/server/configuration)

A full-featured drop-in replacement for [Komga](https://komga.org) in Rust — same library formats, API, and database, plus additional capabilities. It supports CBZ, CBR, PDF, and EPUB, with a bundled web UI in a single static binary — no JVM and a small memory footprint.

*kmrs: Keep Manga Reading Simple.*

- **Drop-in replacement** for `gotson/komga`: same port (25600), same `/config` and `/data` mounts, same `KOMGA_*` environment variables
- **Data-level compatibility**: opens and upgrades existing komga data directories (`database.sqlite`, `tasks.sqlite`) in place — and the Java version can still open libraries written by kmrs
- **Library formats**: CBZ/ZIP, CBR/RAR, PDF, and EPUB — scanning, metadata extraction, thumbnails, and Komga-compatible page/resource serving (PDF rendering uses libpdfium)
- **API parity**: REST `/api/**`, OPDS v1.2/v2, SSE, Kobo sync, and KOReader progress sync — endpoints, DTOs, pagination, error shapes, and authentication behavior match the Java version
- **Verified against the Java version**: byte-for-byte Flyway migrations, a differential test harness comparing ~105 endpoints against a live Java instance, and schema contract tests
- **Efficient by design**: no JVM, bounded memory, and container caches that avoid reopening books for every page
- **Beyond Komga**: smart lists, reading trackers, CJK cross-search, outbound webhooks, reading statistics, and [more](https://kmworks.date/server/enhancements)

## Quick start

```sh
docker run -d \
  --name=komga \
  --user 1000:1000 \
  -p 25600:25600 \
  --mount type=bind,source=/path/to/config,target=/config \
  --mount type=bind,source=/path/to/data,target=/data \
  --restart unless-stopped \
  ghcr.io/kmworks/kmrs
```

The kmweb React web UI (built from [`webui/`](webui/) in this repo) is embedded in the binary and served at `/` out of the box — nothing to configure. [Releases](https://github.com/kmworks/kmrs/releases/latest) provide a signed KMServer app for Apple silicon, a per-user Windows installer, and prebuilt binaries for Linux and Windows; API-only clients like [KMReader](https://github.com/kmworks/kmreader), KOReader, or Kobo work too.

Desktop downloads: [macOS (Apple silicon)](https://github.com/kmworks/kmrs/releases/latest/download/KMServer_aarch64.dmg) · [Windows (x64)](https://github.com/kmworks/kmrs/releases/latest/download/KMServer_x64-setup.exe)

## Documentation

Full documentation lives at **[kmworks.date/server](https://kmworks.date/server/)**:

- [Installation](https://kmworks.date/server/installation) — Docker, KMServer desktop apps, prebuilt binaries, build from source
- [Supported formats](https://kmworks.date/server/intro#supported-formats) — CBZ/ZIP, CBR/RAR, PDF, EPUB, and Webtoon reading
- [Configuration](https://kmworks.date/server/configuration) — `config.toml` key reference, env vars, precedence
- [Serving a web UI](https://kmworks.date/server/webui) — bundled web UI, reverse-proxy setups
- [Search](https://kmworks.date/server/search) — analyzer chain, CJK cross-search extensions
- [Compatibility](https://kmworks.date/server/compatibility) and [Known limitations](https://kmworks.date/server/limitations)
- [Enhancements](https://kmworks.date/server/enhancements) — additional capabilities and intentional behavior differences
- [Development](https://kmworks.date/server/development) — tests, crate structure, heap profiling

## License

kmrs is under the [MIT License](LICENSE). The SQL migration files, the OpenAPI document, and the test fixtures are copied from the [komga](https://github.com/gotson/komga) source tree (see [NOTICE](NOTICE)); everything else is a rewritten implementation. kmrs is not affiliated with the komga project.
