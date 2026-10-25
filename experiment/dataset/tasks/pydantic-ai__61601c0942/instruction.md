You are working in the git repository in the current directory. Make the change described below. Work autonomously: nobody will answer questions.

# Hugging Face model silently drops file content returned by tools

## Problem

`HuggingFaceModel` ignores multimodal content returned from a tool. When a tool returns an `ImageUrl`, a `DocumentUrl`, binary content, or a list that mixes text and files, the request sent to Hugging Face contains only the tool's text rendering. The files never reach the model. Unsupported media types also pass through without any error, even though the same types raise an error when they appear in a user prompt.

## Required behaviour

When a `ModelRequest` that contains `ToolReturnPart`s is mapped into Hugging Face chat messages:

1. **Tool message.** Each `ToolReturnPart` still produces a `role: 'tool'` message with the correct `tool_call_id`. Its `content` must be the text portion of the return, with each file replaced by a textual reference. Use the same split that the public `ToolReturnPart.model_response_str_and_user_content()` already provides. For example, a tool returning only `ImageUrl(url='https://example.com/image.png')` with call id `call_1` gives:
   ```
   {'role': 'tool', 'content': 'See file 01a7df.', 'tool_call_id': 'call_1'}
   ```

2. **Follow-up user message.** The file content from all tool returns in the request is collected. After all of the request's parts have been mapped, emit one additional `role: 'user'` message containing that file content. Build it exactly as a `UserPromptPart` with that content would be mapped. This includes the text labels from the same helper (e.g. `'This is file 01a7df:'`), followed by the file itself (e.g. an `image_url` item with `{'url': 'https://example.com/image.png'}`).

   For the single-image example, the mapped output is:
   - the tool message above, then
   - a user message whose content is:
     - a text item `'This is file 01a7df:'`
     - an `image_url` item for the image.

   If no tool return in the request contains files, no extra user message is emitted.

3. **Unsupported media types.** Media types that Hugging Face does not support must raise `NotImplementedError` when they appear in a tool return, with the same message used for user prompts. For example, a tool returning `['here', DocumentUrl(url='url')]` in the message history must make `agent.run(...)` raise `NotImplementedError` matching `'DocumentUrl is not supported for Hugging Face'`.

Existing behaviour is unchanged for:
- system prompts
- user prompts
- retry prompts
- tool returns that contain only text
