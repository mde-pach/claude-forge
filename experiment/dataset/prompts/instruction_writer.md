You are writing a task statement for a software engineer. They will implement a change in the repository `{repo}` (checked out in their working directory), starting from the state just before an upstream commit. You are given that commit's message and its full diff, both source and tests.

Write the statement the way a good issue or ticket would: what is wrong or missing, and what must be true once the change is done.

Rules:
1. Describe observable behaviour and requirements, not the implementation. Do not tell the engineer which internal files, functions or algorithms to change, unless callers use them as public interface.
2. Hidden tests will check the engineer's work, and the engineer will not see them. Name every **public interface** those tests rely on that is new or changes:
   - import paths;
   - class, function, method and attribute names;
   - parameters and their defaults;
   - return values and types;
   - raised exception types;
   - exact user-visible strings (messages, formats, serialised output) the tests compare.

   State these as requirements of the behaviour. Do not quote, list or describe the tests, and do not say that tests exist.
3. Do not paste code from the diff. Short signatures or short examples of usage are fine.
4. Do not mention the commit, the upstream project's fix, pull requests or issue numbers.
5. Be complete but brief: at most about 400 words. Output only the statement, in Markdown, with no preamble.

Commit message:
```
{message}
```

Diff (source files):
```diff
{src_diff}
```

Diff (test files):
```diff
{test_diff}
```
