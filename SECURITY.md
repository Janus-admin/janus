# Security Policy

## Supported Versions

| Version | Supported |
|---|---|
| Latest (`master`) | Yes |

## Reporting a Vulnerability

**Please do not open a public GitHub issue for security vulnerabilities.**

Email us at: **security@janus-gateway.io** (or the maintainer directly)

Include:
- Description of the vulnerability
- Steps to reproduce
- Potential impact
- Any suggested fix (optional)

You will receive a response within 48 hours. We aim to release a fix within 7 days for critical issues.

## Scope

In scope:
- Authentication bypass on admin or gateway endpoints
- API key leakage or improper storage
- Remote code execution
- SQL injection
- Provider credential exposure

Out of scope:
- Issues in third-party dependencies (report to them directly)
- Denial of service via intentional load
- Issues requiring physical access
