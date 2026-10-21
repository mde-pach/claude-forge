Playwright. Locate elements with `getByRole`, `getByLabel`, `getByText` or `getByTestId`; web-first assertions (`await expect(locator)...`) wait on their own.
Never wait a fixed time (`waitForTimeout`).
If an existing test looks wrong, stop and ask; never edit it to make the code pass.
Accessibility: `@axe-core/playwright` (`new AxeBuilder({ page }).analyze()`) checks pages for WCAG violations in end-to-end tests.
