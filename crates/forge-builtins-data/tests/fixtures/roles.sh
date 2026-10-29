# Forge's default file roles as the TypeScript Forge renders them (better-auth-config left out),
# for the golden tests, which source this with templates/engine/*.sh.
forge_role() {
  case ${1#@} in
    adr) case $2 in docs/adr/*|doc/adr/*|docs/decisions/*) return 0 ;; esac ;;
    alembic-migrations) case $2 in alembic/versions/*|migrations/versions/*) return 0 ;; esac ;;
    angular-tsconfig) case $2 in tsconfig.app.json|*/tsconfig.app.json) return 0 ;; esac ;;
    build-artifacts) case $2 in node_modules/*|*/node_modules/*|__pycache__/*|*/__pycache__/*|*.pyc|*/*.pyc|.next/*|*/.next/*|.nuxt/*|*/.nuxt/*|.svelte-kit/*|*/.svelte-kit/*|.turbo/*|*/.turbo/*|.coverage|*/.coverage|.DS_Store|*/.DS_Store) return 0 ;; esac ;;
    compose) case $2 in compose*.yaml|*/compose*.yaml|compose*.yml|*/compose*.yml|docker-compose*.yaml|*/docker-compose*.yaml|docker-compose*.yml|*/docker-compose*.yml) return 0 ;; esac ;;
    django-migrations) case $2 in migrations/[0-9]*.py|*/migrations/[0-9]*.py) return 0 ;; esac ;;
    django-settings) case $2 in settings.py|*/settings.py|settings/*.py|*/settings/*.py) return 0 ;; esac ;;
    django-templates) case $2 in templates/*.html|templates/*/*.html|*/templates/*.html|*/templates/*/*.html|templates/*.txt|templates/*/*.txt|*/templates/*.txt|*/templates/*/*.txt) return 0 ;; esac ;;
    dockerfile) case $2 in *.dockerignore|*/*.dockerignore) return 1 ;; esac; case $2 in Dockerfile|*/Dockerfile|Dockerfile.*|*/Dockerfile.*|*.Dockerfile|*/*.Dockerfile|Containerfile|*/Containerfile) return 0 ;; esac ;;
    docs) case $2 in docs/*.md|docs/*/*.md|docs/*.mdx|docs/*/*.mdx|docs/*.rst|docs/*/*.rst|doc/*.md|doc/*/*.md|doc/*.mdx|doc/*/*.mdx|doc/*.rst|doc/*/*.rst|README*) return 0 ;; esac ;;
    drizzle-migrations) case $2 in drizzle/*) return 0 ;; esac ;;
    e2e-tests) case $2 in e2e/*|tests/e2e/*) return 0 ;; esac ;;
    env-files) case $2 in .env.example|*/.env.example|.env.sample|*/.env.sample|.env.template|*/.env.template|.env.dist|*/.env.dist) return 1 ;; esac; case $2 in .env|*/.env|.env.*|*/.env.*) return 0 ;; esac ;;
    env-template) case $2 in .env.example|*/.env.example|.env.sample|*/.env.sample|.env.template|*/.env.template|.env.dist|*/.env.dist) return 0 ;; esac ;;
    esm-modules) case $2 in *.mts|*/*.mts|*.mjs|*/*.mjs) return 0 ;; esac ;;
    generated-clients) ;;
    gunicorn-config) case $2 in gunicorn.conf.py|*/gunicorn.conf.py) return 0 ;; esac ;;
    lockfiles) case $2 in uv.lock|bun.lock|bun.lockb|package-lock.json|pnpm-lock.yaml|yarn.lock) return 0 ;; esac ;;
    prisma-migrations) case $2 in prisma/migrations/*) return 0 ;; esac ;;
    procfile) case $2 in Procfile|Procfile.*) return 0 ;; esac ;;
    python-tests) case $2 in test_*.py|*/test_*.py|*_test.py|*/*_test.py|conftest.py|*/conftest.py) return 0 ;; esac ;;
    sql-migrations) case $2 in db/migration/*.sql|db/migration/*/*.sql|supabase/migrations/*.sql|db/migrations/*.sql) return 0 ;; esac ;;
    ts-tests) case $2 in *.test.ts|*/*.test.ts|*.test.tsx|*/*.test.tsx|*.test.mts|*/*.test.mts|*.test.cts|*/*.test.cts|*.test.js|*/*.test.js|*.test.jsx|*/*.test.jsx|*.test.mjs|*/*.test.mjs|*.test.cjs|*/*.test.cjs|*.spec.ts|*/*.spec.ts|*.spec.tsx|*/*.spec.tsx|*.spec.mts|*/*.spec.mts|*.spec.cts|*/*.spec.cts|*.spec.js|*/*.spec.js|*.spec.jsx|*/*.spec.jsx|*.spec.mjs|*/*.spec.mjs|*.spec.cjs|*/*.spec.cjs|__tests__/*|*/__tests__/*) return 0 ;; esac ;;
    typeorm-datasource) ;;
    wrangler-config) case $2 in wrangler.toml|*/wrangler.toml|wrangler.json|*/wrangler.json|wrangler.jsonc|*/wrangler.jsonc) return 0 ;; esac ;;
    wrangler-types) case $2 in worker-configuration.d.ts) return 0 ;; esac ;;
  esac
  return 1
}
