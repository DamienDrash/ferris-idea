import sqlite3
from pathlib import Path

from flask import Flask, jsonify

DB = Path.home() / "ReferenceEnv" / "db" / "inventory.sqlite"
app = Flask(__name__)


@app.get("/stock/value")
def stock_value():
    with sqlite3.connect(DB) as conn:
        rows = conn.execute("select warehouse_id, value_cents from stock_value order by 1").fetchall()
    return jsonify([{"warehouseId": w, "valueCents": v} for w, v in rows])
