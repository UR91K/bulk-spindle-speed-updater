## Features
- Bulk edit spindle speed commands in G-code files
- GUI for easy operation
- Progress tracking and error handling
- Input validation for spindle speed ranges
It works by modifying the S command near the beginning of .tap each tap file.

## Usage
1. Place the application in the directory with your .tap files (WARNING: it searches recursively)
2. Run the application
3. Enter desired spindle speed (RPM)
4. Click "Update Spindle Speeds" or press Enter
5. Confirm the operation (click "Yes" button or press Enter)
6. Wait for completion

## Building
I didn't do much special with the build, just run something to this effect:
```cargo build --package spindle_speed_manager --bin spindle_speed_manager --release```

## Contributing
Pull requests are welcome. For major changes, please open an issue first.

## License
MIT
