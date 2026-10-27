msgspec. Decoding validates against a type: `msgspec.json.decode(raw, type=Model)` (or a typed `msgspec.json.Decoder(Model)`), never an untyped `decode(raw)`.
