using System;
using System.Windows.Forms;
using System.Collections.Specialized;

namespace file2clip
{
    class Program
    {
        [STAThread]
        static void Main(string[] args)
        {
            StringCollection paths = new StringCollection();
            foreach (string s in args) paths.Add(s);
            Clipboard.SetFileDropList(paths);
        }
    }
}
