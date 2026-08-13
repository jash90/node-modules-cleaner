import { formatSize } from '../utils/formatSize';

interface SizeDisplayProps {
  bytes: number;
  className?: string;
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
