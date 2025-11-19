import subprocess
import threading
import time
import sys
import re
import os

def stream_output(process, prefix, event, event_pattern, output_container=None):
    """Reads stdout/stderr from the process and prints it with a prefix."""
    try:
        # We merge stdout and stderr for simplicity in this test runner, 
        # or we can read them separately. rfcat prints status to stderr and data to stdout.
        # Let's read stderr for status.
        for line in process.stderr:
            line = line.strip()
            print(f"[{prefix} STDERR] {line}")
            if event and not event.is_set() and event_pattern in line:
                event.set()
        
    except Exception as e:
        print(f"[{prefix}] Error reading stderr: {e}")

def stream_stdout(process, prefix, output_container):
    """Reads stdout from the process."""
    try:
        for line in process.stdout:
            line = line.strip()
            print(f"[{prefix} STDOUT] {line}")
            if output_container is not None:
                output_container.append(line)
    except Exception as e:
        print(f"[{prefix}] Error reading stdout: {e}")

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
    
    UUID = "87654321-4321-8765-4321-876543210987"
    
    print(f"--- Starting rfcat Test ---")

    # Kill any existing rfcat processes
    subprocess.run(["ssh", SERVER_HOST, "killall rfcat"], stderr=subprocess.DEVNULL, stdout=subprocess.DEVNULL)
    subprocess.run(["ssh", CLIENT_HOST, "killall rfcat"], stderr=subprocess.DEVNULL, stdout=subprocess.DEVNULL)

    # 1. Start Server on ubupi5a
    print(f"Launching Server on {SERVER_HOST}...")
    
    # We use 'serve' to run a command that prints a greeting and then echoes input.
    # This allows us to verify Server -> Client (greeting) and Client -> Server -> Client (echo).
    SERVER_CMD_STR = "sh -c 'echo \"Hello from Server!\"; cat'"
    SERVER_ADDR = "D8:3A:DD:B4:0A:B3"
    
    server_cmd = [
        "ssh", SERVER_HOST,
        f"cd {SERVER_DIR} && cargo run -p bluer-tools --bin rfcat -- serve --profile {UUID} -- {SERVER_CMD_STR}"
    ]
    
    server_process = subprocess.Popen(
        server_cmd,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        bufsize=0 # Unbuffered
    )

    server_ready = threading.Event()
    server_output = []
    
    t_server_stderr = threading.Thread(target=stream_output, args=(server_process, "SERVER", server_ready, "Registered profile"))
    t_server_stderr.daemon = True
    t_server_stderr.start()

    t_server_stdout = threading.Thread(target=stream_stdout, args=(server_process, "SERVER", server_output))
    t_server_stdout.daemon = True
    t_server_stdout.start()

    print("Waiting for server to register profile...")
    if not server_ready.wait(timeout=300):
        print("Timeout waiting for server to start.")
        server_process.terminate()
        sys.exit(1)
    
    print("Server is ready.")

    # 2. Start Client on ubupi5b
    print(f"Launching Client on {CLIENT_HOST}...")
    
    client_cmd = [
        "ssh", CLIENT_HOST,
        f"cd {CLIENT_DIR} && cargo run -p bluer-tools --bin rfcat -- connect {SERVER_ADDR} --profile {UUID}"
    ]
    
    client_process = subprocess.Popen(
        client_cmd,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        bufsize=0 # Unbuffered
    )

    client_connected = threading.Event()
    client_output = []

    # "Connecting profile..." is printed before connection. 
    # "Connect request from ..." is printed on server when connected.
    # Client doesn't print much on success, it just starts the IO loop.
    # But if it fails it prints error.
    # Let's wait for the server to say "Connect from"
    
    server_connected = threading.Event()
    # We need to add another listener to server stderr or just check the output stream?
    # The stream_output function consumes the stream.
    # I should have made stream_output more flexible.
    # But wait, stream_output runs in a loop. I can't easily add another event to it dynamically without changing it.
    # However, I can just rely on sending data.

    t_client_stderr = threading.Thread(target=stream_output, args=(client_process, "CLIENT", client_connected, "Connecting profile..."))
    t_client_stderr.daemon = True
    t_client_stderr.start()

    t_client_stdout = threading.Thread(target=stream_stdout, args=(client_process, "CLIENT", client_output))
    t_client_stdout.daemon = True
    t_client_stdout.start()

    print("Waiting for client to start connecting...")
    if not client_connected.wait(timeout=300):
        print("Timeout waiting for client to start.")
        client_process.terminate()
        server_process.terminate()
        sys.exit(1)

    print("Client started connecting. Waiting for connection establishment...")
    
    # We can check server output for "Connect from"
    # But stream_output is already running for server.
    # We can just wait a bit more, or we can check the output container if we were saving stderr.
    # Since I didn't save server stderr to a list, I can't check it easily without modifying stream_output.
    # But I can just wait a reasonable amount of time now that compilation is done.
    # Compilation is done when "Connecting profile..." appears.
    
    time.sleep(10) # Give it time to actually connect after compilation

    # 3. Send Data
    print("Sending data from Client to Server...")
    TEST_MSG_C2S = "Hello from Client!"
    try:
        client_process.stdin.write(TEST_MSG_C2S + "\n")
        client_process.stdin.flush()
    except Exception as e:
        print(f"Failed to write to client stdin: {e}")

    time.sleep(2)

    # We don't write to server stdin manually anymore.
    # The server program (sh -c '...') sends "Hello from Server!" automatically.
    TEST_MSG_S2C = "Hello from Server!"

    time.sleep(2)

    # 4. Verify
    print("Verifying received data...")
    
    # Client should receive "Hello from Server!" (greeting)
    client_received_greeting = False
    for line in client_output:
        if TEST_MSG_S2C in line:
            client_received_greeting = True
            break

    # Client should receive "Hello from Client!" (echoed by cat)
    client_received_echo = False
    for line in client_output:
        if TEST_MSG_C2S in line:
            client_received_echo = True
            break

    if client_received_greeting:
        print("SUCCESS: Client received greeting from Server.")
    else:
        print("FAILURE: Client did NOT receive greeting from Server.")

    if client_received_echo:
        print("SUCCESS: Client received echoed data from Server.")
    else:
        print("FAILURE: Client did NOT receive echoed data from Server.")

    # Cleanup
    print("Terminating processes...")
    client_process.terminate()
    server_process.terminate()
    
    if client_received_greeting and client_received_echo:
        print("--- Test Passed ---")
        sys.exit(0)
    else:
        print("--- Test Failed ---")
        sys.exit(1)

if __name__ == "__main__":
    run_test()
