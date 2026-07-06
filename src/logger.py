import logging
import os
import sys
from logging.handlers import RotatingFileHandler
from src.files import LOG_FILE

def setup_logging():
    """
    Sets up the logging configuration for the entire application.
    It configures a RotatingFileHandler to write logs to a file,
    and a StreamHandler to print logs to the standard output/error.
    """
    log_level_str = os.environ.get("FLOW_LOG_LEVEL", "INFO").upper()
    numeric_level = getattr(logging, log_level_str, logging.INFO)

    # Format includes timestamp, level, thread name, logger name, and message
    formatter = logging.Formatter(
        fmt="%(asctime)s [%(levelname)s] (%(threadName)s) %(name)s: %(message)s",
        datefmt="%Y-%m-%d %H:%M:%S"
    )

    # Set up root logger
    root_logger = logging.getLogger()
    root_logger.setLevel(numeric_level)

    # Clear existing handlers if any (to avoid duplicates)
    root_logger.handlers.clear()

    # 1. Console Handler
    console_handler = logging.StreamHandler(sys.stdout)
    console_handler.setLevel(numeric_level)
    console_handler.setFormatter(formatter)
    root_logger.addHandler(console_handler)

    # 2. File Handler (Rotating)
    try:
        file_handler = RotatingFileHandler(
            LOG_FILE,
            maxBytes=5 * 1024 * 1024,  # 5MB
            backupCount=3,
            encoding='utf-8'
        )
        file_handler.setLevel(numeric_level)
        file_handler.setFormatter(formatter)
        root_logger.addHandler(file_handler)
    except Exception as e:
        # Fallback if log file cannot be created/opened
        print(f"Warning: Could not configure file logging: {e}", file=sys.stderr)

    logging.info("Logging initialized at level %s. Log file: %s", log_level_str, LOG_FILE)
