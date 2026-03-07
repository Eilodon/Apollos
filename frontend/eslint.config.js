import js from '@eslint/js';
import tseslint from 'typescript-eslint';
import reactPlugin from 'eslint-plugin-react';
import reactHooks from 'eslint-plugin-react-hooks';

export default tseslint.config(
    // Global ignores — don't lint compiled/generated output
    {
        ignores: ['dist/**', 'node_modules/**', 'vite.config.js', 'vite.config.d.ts'],
    },

    // Base JS recommended
    js.configs.recommended,

    // TypeScript strict + stylistic
    ...tseslint.configs.strict,
    ...tseslint.configs.stylistic,

    // React + React Hooks
    {
        plugins: {
            react: reactPlugin,
            'react-hooks': reactHooks,
        },
        rules: {
            ...reactPlugin.configs.recommended.rules,
            ...reactHooks.configs.recommended.rules,

            // React 18+ — không cần import React
            'react/react-in-jsx-scope': 'off',
            'react/prop-types': 'off',

            // TypeScript overrides
            '@typescript-eslint/no-unused-vars': ['warn', { argsIgnorePattern: '^_', varsIgnorePattern: '^_' }],
            '@typescript-eslint/no-explicit-any': 'warn',
            '@typescript-eslint/consistent-type-imports': ['error', { prefer: 'type-imports' }],

            // Hooks
            'react-hooks/rules-of-hooks': 'error',
            'react-hooks/exhaustive-deps': 'warn',
        },
        settings: {
            react: { version: '18' },
        },
    },
);
