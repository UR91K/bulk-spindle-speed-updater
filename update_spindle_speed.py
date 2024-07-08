import os
import pathlib
import sys
import ctypes
import traceback
import msvcrt

def is_admin():
    try:
        return ctypes.windll.shell32.IsUserAnAdmin()
    except OSError:
        traceback.print_exc()
        return False
    except Exception as e:
        print("An unexpected error occurred: ", str(e))
        return False

def update_file_spindle_speed(file_path, spindle_speed):
    updated_lines = []
    found_s_command = False
    with open(file_path, "r") as file:
        for line in file:
            if found_s_command:
                updated_lines.append(line)
            elif line.startswith("S"):
                found_s_command = True
                if "S{} ".format(spindle_speed) in line:
                    # print(f"Matching S command already exists in {file_path.name}, skipping...")
                    return
                else:
                    # print(f"S command found, updating file {file_path.name}")
                    line = "S{} M3\n".format(spindle_speed)
                    updated_lines.append(line)
                    found_s_command = True
            else:
                updated_lines.append(line)
    with open(file_path, "w") as file:
        file.writelines(updated_lines)

def update_spindle_speed(folder_path, spindle_speed):
    total_files = sum(1 for _ in pathlib.Path(folder_path).rglob('*.tap'))
    processed_files = 0
    for root, dirs, files in os.walk(folder_path):
        for file in files:
            if file.endswith(".tap"):
                file_path = pathlib.Path(os.path.join(root, file))
                update_file_spindle_speed(file_path, spindle_speed)
                processed_files += 1
                progress_bar = f"[{'=' * int((processed_files / total_files) * 50):50}] {processed_files}/{total_files}"
                print(f"\rProcessing: {file_path.name}\n{progress_bar}", end="", flush=True)

if __name__ == "__main__":
    spindle_speed = input("Enter the desired spindle speed: ")
    folder_path = pathlib.Path(os.path.dirname(os.path.abspath(sys.argv[0])))
    update_spindle_speed(folder_path, spindle_speed)
    print("\nProcessing complete, press any key to continue...")
    msvcrt.getch()