# Security

Jelly controls a real browser and can expose that control through MCP. Treat authentication, OAuth, tunnel, and browser-control bugs as security-sensitive.

## Reporting

Do not open a public issue for a suspected vulnerability.

**Security contact:** [jelly@fabioflorey.com](mailto:jelly@fabioflorey.com?subject=Jelly%20security%20report&body=Hi%2C%0A%0AI%27d%20like%20to%20report%20a%20security%20issue%20in%20Jelly.%0A%0AAffected%20version%20or%20commit%3A%20%0AImpact%3A%20%0AReproduction%20steps%3A%20%0AAdditional%20details%3A%20%0A%0AThanks.).

Report suspected vulnerabilities privately to that address or to the repository owner through GitHub. Include the affected version/commit, reproduction steps, impact, and any relevant logs with secrets removed.

## Secrets

Never include `.env`, bearer tokens, OAuth secrets, tunnel tokens, Telegram credentials, cookies, or browser profile data in reports.
