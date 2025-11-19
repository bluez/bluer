import subprocess
import threading
import time
import sys

def stream_output(process, prefix):
    """Reads stdout from the process and prints it with a prefix."""
    try:
        for line in process.stdout:
            print(f"[{prefix}] {line.strip()}")
    except Exception as e:
        print(f"[{prefix}] Error reading output: {e}")

def run_test():
    # Configuration
    ADV_HOST = "ubupi5a.srv"
    SCAN_HOST = "ubupi5b.srv"
    
    if len(sys.argv) >= 3:
        ADV_DIR = sys.argv[1]
        SCAN_DIR = sys.argv[2]
    else:
        ADV_DIR = "dev/bluer"
        SCAN_DIR = "dev/bluer"

    print(f"Using Advertiser Dir: {ADV_DIR}")
    print(f"Using Scanner Dir:    {SCAN_DIR}")
    
    # Increase timeouts as requested by user
    ADV_DURATION = "60s"
    SCAN_DURATION = "45s"

    print(f"--- Starting Bluetooth LE Advertising Test ---")

    # 1. Start Advertiser on ubupi5a
    # We use 'timeout' on the remote machine to ensure it stops eventually.
    print(f"Launching Advertiser on {ADV_HOST}...")
    adv_cmd = [
        "ssh", ADV_HOST,
        f"cd {ADV_DIR} && timeout {ADV_DURATION} cargo run --example le_advertise"
    ]
    
    # Popen starts the process in the background (from Python's perspective)
    # stdout=subprocess.PIPE allows us to capture the output
    # stderr=subprocess.STDOUT merges stderr into stdout
    adv_process = subprocess.Popen(
        adv_cmd, 
        stdout=subprocess.PIPE, 
        stderr=subprocess.STDOUT, 
        text=True, 
        bufsize=1
    )
    
    # Start a thread to print advertiser output in real-time
    adv_thread = threading.Thread(target=stream_output, args=(adv_process, "ADV"))
    adv_thread.daemon = True
    adv_thread.start()

    # 2. Wait for the advertiser to initialize
    print("Waiting 5 seconds for advertiser to start...")
    time.sleep(5)

    # 3. Start Scanner on ubupi5b
    print(f"Launching Scanner on {SCAN_HOST}...")
    scan_cmd = [
        "ssh", SCAN_HOST,
        f"cd {SCAN_DIR} && timeout {SCAN_DURATION} cargo run --example discover_devices"
    ]
    
    scan_process = subprocess.Popen(
        scan_cmd, 
        stdout=subprocess.PIPE, 
        stderr=subprocess.STDOUT, 
        text=True, 
        bufsize=1
    )
    
    # Start a thread to print scanner output in real-time
    scan_thread = threading.Thread(target=stream_output, args=(scan_process, "SCAN"))
    scan_thread.daemon = True
    scan_thread.start()

    # 4. Wait for the scanner to finish (it determines the test duration)
    exit_code = scan_process.wait()
    print(f"Scanner finished with exit code {exit_code}")
    
    # 5. Cleanup Advertiser
    if adv_process.poll() is None:
        print("Terminating advertiser...")
        adv_process.terminate()
        try:
            adv_process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            print("Advertiser did not exit, killing...")
            adv_process.kill()
    
    # Wait for advertiser thread to finish reading output
    adv_thread.join(timeout=1)
    print("--- Test Finished ---")

if __name__ == "__main__":
    run_test()
