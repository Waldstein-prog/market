#!/bin/bash
set -e
cd "$(dirname "$0")/backend"
[ -d venv ] || python3 -m venv venv
./venv/bin/pip install -q -r requirements.txt
PORT=8700 ./venv/bin/python app.py
