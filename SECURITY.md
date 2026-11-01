# Security Policy

## Supported versions

Security fixes are made for the latest release only.

## What is security-relevant here

- „Für das System installieren“ hands a staging directory to the root helper
  `epson-p900-media-install` of [epson-p900-escpr-cups](https://github.com/colorcube/epson-p900-escpr-cups)
  via `pkexec`. Issues in the helper itself belong to that project.
- `epmi` parses `.emx` / `.epmi` archives (also downloaded from Epson's media server over HTTPS)
  and talks SNMP to the printer, including writes to its custom media slots.

## Reporting a vulnerability

Please **do not open a public issue**. Use GitHub's private vulnerability reporting instead:
[Report a vulnerability](https://github.com/colorcube/ep-media-installer/security/advisories/new).

Include the affected version, steps to reproduce and, if possible, a proof of concept.
This is a spare-time project: reports are answered as soon as possible, usually within a few weeks.
