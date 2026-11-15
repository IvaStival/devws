# Backend conventions

Adapted from maestro; follow the target repository's real architecture.
Keep entry points thin and business services cohesive. Depend on stable interfaces
through constructor injection. Use typed DTOs and existing repository patterns.
Define transaction boundaries and explicit integration failures; asynchronous jobs
must be retry-safe and idempotent. Reuse configured authenticated integration
clients. Validate input and tenant/authorization boundaries before accessing data.
Never expose credentials or sensitive payloads. Cover failure and access cases.
Do not run destructive migrations or reset existing databases. Request a specific
leader action and wait for explicit user approval of the target and environment.
