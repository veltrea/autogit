#!/bin/bash

# AutoGit Public Release Script
# このスクリプトは、現在の開発履歴（auto update等）をすべて消去し、
# 「Initial public release」という1つのクリーンなコミットとしてGitHubに公開します。

PUBLIC_REPO_URL="https://github.com/veltrea/autogit"

# 1. 念のため現在の作業をすべて追加
git add .

# 2. 新しい一時的な「孤立した（履歴のない）」ブランチを作成
echo "Creating a clean branch without history..."
git checkout --orphan temp_branch

# 3. すべてのファイルをステージング
git add -A

# 4. 公開用の最初のコミットを作成
echo "Committing initial release..."
git commit -m "Initial public release of AutoGit"

# 5. 古いメインブランチ（履歴あり）をローカルから削除し、一時ブランチをmainに昇格
git branch -D main 2>/dev/null
git branch -D master 2>/dev/null
git branch -m main

# 6. リモートの設定（既存のoriginがある場合は差し替え、なければ追加）
if git remote | grep -q "^origin$"; then
    echo "Updating origin to public repository: $PUBLIC_REPO_URL"
    git remote set-url origin "$PUBLIC_REPO_URL"
else
    echo "Adding origin: $PUBLIC_REPO_URL"
    git remote add origin "$PUBLIC_REPO_URL"
fi

# 7. 強制プッシュ（GitHub上の履歴を上書きして、クリーンな1コミットにする）
echo "Pushing to GitHub (Force push)..."
git push -f origin main

echo ""
echo "[V] Successfully published to $PUBLIC_REPO_URL without history!"
echo "    これで公開用のリポジトリが1つの綺麗なコミットからスタートします。"
