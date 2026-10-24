## Organization plugin: `cancelPendingInvitationsOnReInvite` has no effect — re-inviting returns 400

### Problem

The organization plugin has an option, `cancelPendingInvitationsOnReInvite`. When it is enabled, inviting an email that already has a pending invitation should cancel the old invitation and create a new one.

The option currently never takes effect. When a user is invited a second time without `resend: true`, the invite endpoint rejects the request with `400 BAD_REQUEST` and the `USER_IS_ALREADY_INVITED_TO_THIS_ORGANIZATION` error. This happens before the cancellation logic can run, so the option cannot be reached.

### Expected behaviour

**With `organization({ cancelPendingInvitationsOnReInvite: true })`:**

Inviting the same email to the same organization a second time (`client.organization.inviteMember({ organizationId, email, role })`, without `resend`) must:

- succeed, with no error;
- return a new invitation whose `status` is `"pending"` and whose `id` differs from the original invitation's `id`;
- leave exactly one `pending` invitation for that email in `client.organization.listInvitations()`;
- mark the previous invitation for that email as `"canceled"`, so exactly one canceled invitation appears in the list.

**With the option disabled (the default, e.g. `organization()`):**

The behaviour stays as it is now. Re-inviting an email that has a pending invitation, without `resend`, must fail with the `ORGANIZATION_ERROR_CODES.USER_IS_ALREADY_INVITED_TO_THIS_ORGANIZATION` error. The error message must equal that code's `.message`.

**Unchanged:**

Re-inviting with `resend: true` must keep its existing behaviour of reusing the existing invitation, whatever the value of `cancelPendingInvitationsOnReInvite`.
