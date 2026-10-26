NestJS: the global `ValidationPipe` is created with `whitelist: true` (and `forbidNonWhitelisted: true`), so undeclared body fields never reach a handler.
`@Body()`, `@Query()` and `@Param()` without a pipe argument are typed with a DTO class (class-validator decorators); `any`, `unknown`, `object`, inline object types and `Record<…>` are rejected, since only classes are validated.
A nested DTO property has `@ValidateNested()` together with `@Type(() => Child)` from class-transformer.
Controllers throw `HttpException` subclasses (`NotFoundException`, `BadRequestException`, …), not a plain `Error`.
