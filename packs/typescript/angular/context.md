Angular 20+: templates use built-in control flow (`@if`, `@for (x of xs; track x.id)`, `@switch`); `*ngIf`, `*ngFor` and `[ngSwitch]` are deprecated and rejected (`ng generate @angular/core:control-flow` migrates them).
`DomSanitizer.bypassSecurityTrust*` turns off Angular's XSS protection and is rejected; bind values and let Angular sanitise them.
