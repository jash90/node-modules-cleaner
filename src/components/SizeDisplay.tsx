interface SizeDisplayProps {
  bytes: number;
  className?: string;
}

export function formatSize(bytes: number): string {
  if (bytes === 0) return '0 B';

  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  const k = 1024;
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  const size = bytes / Math.pow(k, i);

  return `${size.toFixed(i > 0 ? 1 : 0)} ${units[i]}`;
}

export function SizeDisplay({ bytes, className = '' }: SizeDisplayProps) {
  const formatted = formatSize(bytes);

  // Color code based on size
  let colorClass = 'text-gray-600';
  if (bytes > 500 * 1024 * 1024) { // > 500MB
    colorClass = 'text-red-600 font-semibold';
  } else if (bytes > 100 * 1024 * 1024) { // > 100MB
    colorClass = 'text-orange-600';
  } else if (bytes > 50 * 1024 * 1024) { // > 50MB
    colorClass = 'text-yellow-600';
  }

  return (
    <span className={`${colorClass} ${className}`}>
      {formatted}
    </span>
  );
}
