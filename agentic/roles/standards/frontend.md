# Frontend conventions

Adapted from orbit-front; verify the target repository's installed stack.
Components own presentation and UI events. Business hooks orchestrate behavior.
Domain services use the established authenticated API client. Validate boundary
payloads with the project's schema system and derive types from those schemas.
Reuse existing UI components and use granular store subscriptions. Keep loading,
empty, error and access-denied states explicit. Test behavior and accessibility.
Authorization must also be enforced server-side. Do not copy project-specific
paths, API clients, framework versions or business rules into another repository.
