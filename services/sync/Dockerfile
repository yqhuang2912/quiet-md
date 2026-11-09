FROM python:3.13-slim
WORKDIR /app
COPY server.py .
ENV PYTHONUNBUFFERED=1 SYNC_DATA=/data
RUN useradd --system --uid 10001 syncuser && mkdir /data && chown syncuser /data
USER syncuser
VOLUME /data
EXPOSE 8787
HEALTHCHECK --interval=30s --timeout=5s CMD python -c "import urllib.request; urllib.request.urlopen('http://127.0.0.1:8787/health', timeout=3)"
CMD ["python", "server.py"]
