The API client is generated from the OpenAPI document by a package.json script (openapi-typescript, orval or @hey-api/openapi-ts); generated files say so in their header.
A change to a tracked OpenAPI document without a change to a generated file is rejected at Stop: run the generator script and keep both changes together.
A change to a generated file without a change to the document or the generator config is rejected at Stop: change the document (or the server it comes from) and regenerate.
