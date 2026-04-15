import { type ClassValue, clsx } from 'clsx'
import { twMerge } from 'tailwind-merge'

export type { WithElementRef, WithoutChildrenOrChild } from 'bits-ui'

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs))
}
