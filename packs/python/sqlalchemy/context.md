SQLAlchemy 2.0 async. Models: `class Base(DeclarativeBase)`, `Mapped[T]`, `mapped_column()`, `relationship(back_populates=...)`. Queries: `select()` with `session.execute()` or `session.scalars()`. Sessions: `async_sessionmaker(engine, expire_on_commit=False)`.
Async sessions cannot lazy-load (`MissingGreenlet`): load related rows with `selectinload()`, or declare `lazy="raise"`.
`text()` SQL is constant with bound parameters (`:id`).
