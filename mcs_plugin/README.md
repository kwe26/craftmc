# DealSign — CraftMC Paper plugin

In-game commands wired to the manager's public `/mcsapi/*` endpoints.

## Build

Requires JDK 21 (Paper 1.21+ ships Java 21 bytecode).

```
cd mcs_plugin
gradle build
```

Output: `build/libs/DealSign.jar`. Drop it into your server's `plugins/` directory.

If you don't have Gradle installed, the GitHub Actions workflow at
`.github/workflows/build.yml` builds it for you on every push and attaches it
to releases on `v*` tags.

## Configure

After first run a `plugins/DealSign/config.yml` is created:

```yaml
api-base: "http://localhost:3000"
timeout-seconds: 10
```

Set `api-base` to the URL of your CraftMC manager (no trailing slash).

## Commands

| Command | Description |
| --- | --- |
| `/sign <dealId>` | Sign a deal (calls `/mcsapi/record/approve`). |
| `/reject <dealId> [reason...]` | Reject a deal (calls `/mcsapi/record/reject`). |
| `/deal <dealId>` | Show a deal's title, status, and parties (calls `/mcsapi/record/view`). |
| `/deals` | List deals where you are a party and the status is not `signed` / `rejected`. |
