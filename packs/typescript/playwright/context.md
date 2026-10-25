Playwright. Locate elements with `getByRole`, `getByLabel`, `getByText` or `getByTestId`; web-first assertions (`await expect(locator)...`) wait on their own.
`waitForTimeout` (a fixed wait) is rejected in end-to-end tests.
When the task changes behaviour an existing test asserts, update that test in place with Edit or Write (keep its name; Forge asks for approval) and say so in your summary. Otherwise never edit an existing test to make the code pass.
Accessibility: `@axe-core/playwright` (`new AxeBuilder({ page }).analyze()`) checks pages for WCAG violations in end-to-end tests.
