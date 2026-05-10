import { type ClassValue, clsx } from 'clsx'
import { twMerge } from 'tailwind-merge'

export type { WithElementRef, WithoutChildren, WithoutChildrenOrChild } from 'bits-ui'

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs))
}
