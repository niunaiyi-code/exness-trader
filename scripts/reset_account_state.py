import sqlite3

conn = sqlite3.connect('gl.db')
cursor = conn.cursor()
cursor.execute("UPDATE accounts SET state='init' WHERE account_id='18000069657'")
conn.commit()
cursor.execute("SELECT account_id, state FROM accounts")
print(cursor.fetchall())
