"""users"""

import sqlalchemy as sa
from alembic import op

revision = "0001"
down_revision = None


def upgrade() -> None:
    op.create_table("users", sa.Column("id", sa.Integer, primary_key=True), sa.Column("email", sa.String, unique=True, nullable=False))


def downgrade() -> None:
    op.drop_table("users")
