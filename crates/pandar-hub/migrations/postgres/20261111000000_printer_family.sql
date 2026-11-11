ALTER TABLE printers ADD COLUMN printer_family TEXT NOT NULL DEFAULT 'bambu_lab'
    CHECK (printer_family IN ('bambu_lab', 'moonraker'));
