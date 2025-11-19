import subprocess
import threading
import time
import sys
import re

def stream_output(process, prefix, address_event, address_container):
    """Reads stdout from the process and prints it with a prefix."""
    try:
        for line in process.stdout:
            line = line.strip()
            print(f"[{prefix}] {line}")
            # Listening on 5C:F3:70:A0:3B:93. Press enter to quit.
            if "Listening on" in line and address_event and not address_event.is_set():
                match = re.search(r"Listening on ([0-9A-F:]+)\.", line)
                if match:
                    addr = match.group(1)
                    print(f"[{prefix}] Detected Address: {addr}")
                    address_container.append(addr)
                    address_event.set()
            
    except Exception as e:
        print(f"[{prefix}] Error reading output: {e}")

def run_test():
    # Configuration
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
    print("Ensure code is synced to remote machines before running this test.")
    
    print(f"--- Starting RFCOMM Test ---")

    # 1. Start Server on ubupi5a
    print(f"Launching Server on {SERVER_HOST}...")
    
    server_cmd = [
        "ssh", SERVER_HOST,
        f"cd {SERVER_DIR} && cargo run --example rfcomm_profile_server"
    ]
    
    server_process = subprocess.Popen(
        server_cmd, 
        stdout=subprocess.PIPE, 
        stderr=subprocess.STDOUT, 
        stdin=subprocess.PIPE, # Keep stdin open so it doesn't exit immediately
        text=True, 
        bufsize=1
    )
    
    address_event = threading.Event()
    address_container = []
    
    # Start a thread to print server output in real-time
    server_thread = threading.Thread(target=stream_output, args=(server_process, "SERVER", address_event, address_container))
    server_thread.daemon = True
    server_thread.start()

    # 2. Wait for the server to initialize and print address
    print("Waiting for server to start and print address...")
    if not address_event.wait(timeout=60):
        print("Timeout waiting for server address.")
        server_process.terminate()
        return

    server_addr = address_container[0]
    print(f"Server Address: {server_addr}")

    # 3. Start Client on ubupi5b
    print(f"Launching Client on {CLIENT_HOST}...")
    client_cmd = [
        "ssh", CLIENT_HOST,
        f"cd {CLIENT_DIR} && cargo run --example rfcomm_profile_client {server_addr}"
    ]
    
    client_process = subprocess.Popen(
        client_cmd, 
        stdout=subprocess.PIPE, 
        stderr=subprocess.STDOUT, 
        text=True, 
        bufsize=1
    )
    
    # Start a thread to print client output in real-time
    client_thread = threading.Thread(target=stream_output, args=(client_process, "CLIENT", None, None))
    client_thread.daemon = True
    client_thread.start()

    # 4. Wait for the client to finish
    exit_code = client_process.wait()
    print(f"Client finished with exit code {exit_code}")
    
    # 5. Cleanup Server
    print("Terminating server...")
    server_process.terminate()
    try:
        server_process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        print("Server did not exit, killing...")
        server_process.kill()
    
    server_thread.join(timeout=1)
    print("--- Test Finished ---")

if __name__ == "__main__":
    run_test()
