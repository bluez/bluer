import subprocess
import threading
import time
import sys

SCRIPT_START_TIME = time.time()

def stream_output(process, prefix):
    """Reads stdout from the process and prints it with a prefix."""
    try:
        for line in process.stdout:
            elapsed = time.time() - SCRIPT_START_TIME
            print(f"[{elapsed:7.2f}s] [{prefix}] {line.strip()}")
    except Exception as e:
        print(f"[{prefix}] Error reading output: {e}")

def run_test_pair(server_host, client_host, server_example, client_example, server_dir, client_dir):
    print(f"\n=== Testing {server_example} (Server) <-> {client_example} (Client) ===")
    
    # Increase timeouts
    SERVER_DURATION = "1200s"
    CLIENT_DURATION = "1200s"

    # 1. Start Server
    print(f"Launching Server ({server_example}) on {server_host}...")
    server_cmd = [
        "ssh", server_host,
        f"cd {server_dir} && timeout {SERVER_DURATION} cargo run --example {server_example}"
    ]
    
    server_process = subprocess.Popen(
        server_cmd, 
        stdout=subprocess.PIPE, 
        stderr=subprocess.STDOUT, 
        text=True, 
        bufsize=1
    )
    
    server_thread = threading.Thread(target=stream_output, args=(server_process, "SERVER"))
    server_thread.daemon = True
    server_thread.start()

    # 2. Wait for server to initialize
    print("Waiting 5 seconds for server to start...")
    time.sleep(5)

    # 3. Start Client
    print(f"Launching Client ({client_example}) on {client_host}...")
    client_cmd = [
        "ssh", client_host,
        f"cd {client_dir} && timeout {CLIENT_DURATION} cargo run --example {client_example}"
    ]
    
    client_process = subprocess.Popen(
        client_cmd, 
        stdout=subprocess.PIPE, 
        stderr=subprocess.STDOUT, 
        text=True, 
        bufsize=1
    )
    
    client_thread = threading.Thread(target=stream_output, args=(client_process, "CLIENT"))
    client_thread.daemon = True
    client_thread.start()

    # 4. Wait for client to finish
    exit_code = client_process.wait()
    print(f"Client finished with exit code {exit_code}")
    
    # 5. Cleanup Server
    if server_process.poll() is None:
        print("Terminating server...")
        server_process.terminate()
        try:
            server_process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            print("Server did not exit, killing...")
            server_process.kill()
    
    server_thread.join(timeout=1)
    
    if exit_code != 0:
        print(f"!!! Test Failed: {server_example} <-> {client_example} !!!")
        return False
    else:
        print(f"+++ Test Passed: {server_example} <-> {client_example} +++")
        return True

def main():
    SERVER_HOST = "ubupi5a.srv"
    CLIENT_HOST = "ubupi5b.srv"
    
    if len(sys.argv) >= 3:
        SERVER_DIR = sys.argv[1]
        CLIENT_DIR = sys.argv[2]
    else:
        SERVER_DIR = "dev/bluer"
        CLIENT_DIR = "dev/bluer"

    print(f"Using Server Dir: {SERVER_DIR}")
    print(f"Using Client Dir: {CLIENT_DIR}")

    tests = [
        # ("gatt_server_cb", "gatt_client"),
        # ("gatt_server_io", "gatt_client"),
        ("gatt_echo_server", "gatt_echo_client"),
    ]

    results = []
    for server_ex, client_ex in tests:
        success = run_test_pair(SERVER_HOST, CLIENT_HOST, server_ex, client_ex, SERVER_DIR, CLIENT_DIR)
        results.append((server_ex, client_ex, success))
        time.sleep(2) # Cool down

    print("\n=== Summary ===")
    all_passed = True
    for server_ex, client_ex, success in results:
        status = "PASS" if success else "FAIL"
        print(f"{server_ex} <-> {client_ex}: {status}")
        if not success:
            all_passed = False
    
    if not all_passed:
        sys.exit(1)

if __name__ == "__main__":
    main()
