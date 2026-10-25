Django REST framework. A `ModelSerializer`'s `Meta.fields` lists the fields; `fields = "__all__"` is rejected. Fields the client must not set are in `read_only_fields`.
Views that subclass a DRF view or viewset directly set `permission_classes` unless settings set a non-`AllowAny` `DEFAULT_PERMISSION_CLASSES` (DRF's default is `AllowAny`); `AllowAny` on a write endpoint, or as the default, is rejected: ask the user first.
Querysets behind list endpoints and nested serializers use `select_related()` / `prefetch_related()`.
