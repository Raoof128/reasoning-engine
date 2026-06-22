FROM python:3.12.11-slim

ENV PYTHONDONTWRITEBYTECODE=1
ENV PYTHONUNBUFFERED=1
ENV REASONING_ENGINE_DB=/data/reasoning.db
ENV REASONING_ENGINE_RUNS_DIR=/data/runs

WORKDIR /app

COPY pyproject.toml README.md ./
COPY src ./src

RUN pip install --no-cache-dir --upgrade pip \
    && pip install --no-cache-dir .

RUN useradd --create-home appuser \
    && mkdir -p /data/runs \
    && chown -R appuser:appuser /app /data
USER appuser

EXPOSE 8765

HEALTHCHECK --interval=30s --timeout=5s --retries=3 \
    CMD python -c "import os, socket; socket.create_connection(('127.0.0.1', int(os.environ.get('PORT', '8765'))), timeout=3).close()"

CMD ["sh", "-c", "reasoning-engine serve --transport http --host 0.0.0.0 --port ${PORT:-8765} --unsafe-bind-public --bearer-token-env REASONING_ENGINE_HTTP_TOKEN"]
