import requests
import time

login_resp = requests.post("http://127.0.0.1:8080/api/v1/auth/login", json={"username": "18000069657", "password": "Password123!"})
token = login_resp.json().get("token")

for i in range(3):
    t0 = time.time()
    r = requests.get("http://127.0.0.1:8080/api/v1/accounts/18000069657/snapshot", headers={"Authorization": f"Bearer {token}"})
    dt = time.time() - t0
    print(f"Call {i+1}: status={r.status_code}, latency={dt:.2f}s, state={r.json().get('account_state')}")
    time.sleep(1)
