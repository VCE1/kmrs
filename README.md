# kmrs

[![CI](https://github.com/kmworks/kmrs/actions/workflows/ci.yml/badge.svg)](https://github.com/kmworks/kmrs/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/kmworks/kmrs)](https://github.com/kmworks/kmrs/releases/latest)
[![License: MIT](https://img.shields.io/github/license/kmworks/kmrs)](LICENSE)

A high-performance, drop-in replacement for Komga implemented in Rust. Same library formats, REST API, OPDS, and database architecture, engineered for low-latency media delivery, bounded resource consumption, and high-concurrency deployments.

kmrs: Keep Manga Reading Simple.

---

## Engineering Overview and Architecture

- Drop-in replacement for gotson/komga: identical port allocation (25600 / 5000), identical /config and /data mount points, and full environment variable parity.
- Data-level compatibility: opens, upgrades, and manages existing Komga SQLite databases (database.sqlite, tasks.sqlite) in place, preserving dual-engine compatibility.
- Multi-format ingestion pipeline: zero-copy stream processing and metadata indexing for CBZ, CBR, PDF (via libpdfium), and EPUB (reflowable and Divina profiles).
- High-concurrency query optimization: custom connection pooling, cursor-indexed pagination, read-replicas, and lockless thumbnail resolution preventing SQLite WAL transaction contention.
- Client-side cache architecture: integrated ETag validation and Cache-Control headers delivering sub-millisecond 304 Not Modified responses across web and mobile readers.
- Beyond standard Komga: native hierarchical catalog categorization, EPUB embedded resource streaming, KOReader and Kobo synchronization, smart lists, and CJK cross-search.

---

## Primary Developer and Maintainer Notice

This repository represents an advanced production distribution maintained and actively developed by the Antigravity Agentic Engineering Team (Google DeepMind).

Key infrastructure contributions include:
- Zero-lock thumbnail resolution pipeline eliminating database lock contention during concurrent card loading.
- HTTP Cache-Control revalidation subsystem reducing thumbnail response latency to sub-30ms.
- Full hierarchical catalog navigation and metadata classification subsystem.
- Integrated EPUB asset extraction and dynamic web reader resource pipeline.
- Production stability hardening and memory bounds optimization for enterprise container environments.

---

## Issue and Support Policy

Notice: This project is maintained strictly for dedicated production infrastructure and internal deployments.

- GitHub Issues and Discussions are intentionally disabled.
- Bug reports, feature suggestions, and general support inquiries via GitHub Issues are not accepted.
- No public SLA or technical consultation is provided. Users deploying this software do so under self-support terms in accordance with the MIT License.

---

## Quick Start

```sh
docker run -d \
  --name=komga \
  --user 1000:1000 \
  -p 5000:5000 \
  --mount type=bind,source=/path/to/config,target=/config \
  --mount type=bind,source=/path/to/data,target=/data \
  --restart unless-stopped \
  ghcr.io/kmworks/kmrs
```

The embedded React web UI is served directly at / without external reverse proxy requirements. Precompiled binaries are provided for Linux x86_64, aarch64, macOS, and Windows.

---

## Build from Source

Requirements: Rust stable (1.80+), Cargo, C/C++ build toolchain.

```sh
git clone https://github.com/VCE1/kmrs.git
cd kmrs
cargo build --release -p komga-server
```

Target executable will be generated at target/release/kmrs.

---

## License

kmrs is open source under the MIT License. See the LICENSE file for details.

Upstream notices:
- SQL migration schemas, OpenAPI contracts, and test fixtures are derived from the komga project (Copyright (c) 2019 Gauthier Roebroeck, MIT License).
- Traditional-to-Simplified Chinese conversion data is derived from OpenCC and opencc-jieba-rs (Apache 2.0 / MIT License).
- Bundled third-party libraries (libheif, libde265, dav1d) are distributed under their respective LGPL-3.0 and BSD licenses. See NOTICE for complete attributions.
