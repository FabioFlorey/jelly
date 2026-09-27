# Security

Jelly controls a real browser and can expose that control through MCP. Treat authentication, OAuth, tunnel, and browser-control bugs as security-sensitive.

## Reporting

Do not open a public issue for a suspected vulnerability.

Report it privately to the repository owner through GitHub. Include the affected version/commit, reproduction steps, impact, and any relevant logs with secrets removed.

## Secrets

Never include `.env`, bearer tokens, OAuth secrets, tunnel tokens, Telegram credentials, cookies, or browser profile data in reports.
