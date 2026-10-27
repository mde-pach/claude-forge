ESLint 9+ reads only the flat config `eslint.config.*`: ignores go in its `ignores`, globals in `languageOptions.globals`; `.eslintrc*`, `.eslintignore` and `/* eslint-env */` comments are rejected.
typescript-eslint supports TypeScript below 6.1 (it needs the compiler API, which TypeScript 7 does not ship): the `typescript` it resolves must be 6.x or older.
