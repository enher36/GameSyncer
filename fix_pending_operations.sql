-- Fix script for pending operations
-- This should be run against the SQLite database to fix Pending status operations

-- Update all Pending operations to Completed if they have file_size set
-- (indicating the upload actually succeeded)
UPDATE cloud_operations 
SET status = 'completed',
    completed_at = datetime('now')
WHERE status = 'pending' 
  AND file_size IS NOT NULL 
  AND file_size > 0;

-- Also update progress to 1.0 for completed operations
UPDATE cloud_operations 
SET progress = 1.0
WHERE status = 'completed' 
  AND progress IS NULL;

-- Show summary of operations after fix
SELECT 
    status,
    COUNT(*) as count,
    AVG(file_size) as avg_size_bytes,
    MIN(started_at) as earliest,
    MAX(started_at) as latest
FROM cloud_operations 
GROUP BY status;